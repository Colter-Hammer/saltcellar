//! A fake SimpleFIN Bridge for tests and local development (see `README.md`).
//!
//! It speaks the two calls the app makes: `POST /claim/<token>` answers an access URL, and
//! `GET /accounts` answers the feed, with the `version=2`, `pending=1` and `start-date` parameters
//! honoured the way the live Bridge does. What the feed holds is a named [`Scenario`]. Every bank,
//! account, payee and amount in it is made up.

mod feed;

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};

use axum::Router;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

pub use feed::NOW;

/// What the feed holds, selected per test (or with `--scenario` on the binary).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    /// Two banks, three accounts, a month of posted rows, nothing wrong.
    Steady,
    /// A pending row; from day 1 the same id is posted, with the same amount.
    PendingPosts,
    /// A pending row; from day 1 a posted row with a new id and a larger amount (a tip).
    PendingAmountChanges,
    /// A posted row; from day 1 the same row under a new id.
    IdReissued,
    /// Two equal posted rows a day apart; from day 1 both under new ids, so neither new id can
    /// be told apart.
    IdsReissuedTogether,
    /// A pending row (a hotel deposit); from day 1 it is gone, never posted.
    PendingDropped,
    /// A posted row ten days old; from day 1 the bank stops sending it.
    PostedDropped,
    /// From day 1: rows the household may have entered by hand first, a check and a payment app
    /// transfer.
    ManualRowsArrive,
    /// From day 1: a payment app row a day old, larger than the $50.00 the household may have
    /// entered by hand for it (a possible manual match).
    ManualMatchArrives,
    /// The second bank's link needs re-authorizing: `con.auth` in the error list, its accounts
    /// with a `balance-date` six days old.
    NeedsAuth,
    /// `GET /accounts` answers 403: the access URL was revoked.
    Revoked,
    /// `GET /accounts` answers 402: the subscription is unpaid.
    Unpaid,
    /// `act.missingdata` for the card, which comes without rows.
    MissingData,
    /// The savings account isn't in the response, and the error list is empty.
    AccountMissing,
    /// `gen.auth` in the error list of a 200 answer: the access URL was disabled.
    GenAuth,
    /// `con.failed` for the second bank's link: a bank link error that isn't `con.auth`.
    ConError,
    /// A developer-facing `gen.api` notice with no bank link or account, and good data.
    GenApi,
    /// Everything fine, except the second bank's accounts have a `balance-date` three days old.
    StaleData,
    /// From day 1, the card's balance is reported positive while its rows say it owes money.
    /// `balance-date` moves a day each day (two days old on day 0, now from day 2).
    WrongSign,
    /// From day 1, a $45.00 checking row arrives and the bank's balance drops $54.50: $9.50 of
    /// drift. `balance-date` moves a day each day (two days old on day 0, now from day 2), so day 2
    /// is the second snapshot at that drift and flags it.
    Drift,
    /// A fourth account, `ACT-demo-travel` at the second bank, reports its money in another
    /// currency (CAD, or USD when the fake's own currency is CAD).
    ForeignCurrency,
}

impl Scenario {
    /// Every scenario, in the order the README lists them.
    pub const ALL: [Scenario; 21] = [
        Scenario::Steady,
        Scenario::PendingPosts,
        Scenario::PendingAmountChanges,
        Scenario::IdReissued,
        Scenario::IdsReissuedTogether,
        Scenario::PendingDropped,
        Scenario::PostedDropped,
        Scenario::ManualRowsArrive,
        Scenario::ManualMatchArrives,
        Scenario::NeedsAuth,
        Scenario::Revoked,
        Scenario::Unpaid,
        Scenario::MissingData,
        Scenario::AccountMissing,
        Scenario::GenAuth,
        Scenario::ConError,
        Scenario::GenApi,
        Scenario::StaleData,
        Scenario::WrongSign,
        Scenario::Drift,
        Scenario::ForeignCurrency,
    ];

    /// The name used on the command line and in the README.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Scenario::Steady => "steady",
            Scenario::PendingPosts => "pending-posts",
            Scenario::PendingAmountChanges => "pending-amount-changes",
            Scenario::IdReissued => "id-reissued",
            Scenario::IdsReissuedTogether => "ids-reissued-together",
            Scenario::PendingDropped => "pending-dropped",
            Scenario::PostedDropped => "posted-dropped",
            Scenario::ManualRowsArrive => "manual-rows-arrive",
            Scenario::ManualMatchArrives => "manual-match-arrives",
            Scenario::NeedsAuth => "needs-auth",
            Scenario::Revoked => "revoked",
            Scenario::Unpaid => "unpaid",
            Scenario::MissingData => "missing-data",
            Scenario::AccountMissing => "account-missing",
            Scenario::GenAuth => "gen-auth",
            Scenario::ConError => "con-error",
            Scenario::GenApi => "gen-api",
            Scenario::StaleData => "stale-data",
            Scenario::WrongSign => "wrong-sign",
            Scenario::Drift => "drift",
            Scenario::ForeignCurrency => "foreign-currency",
        }
    }

    /// The scenario called `name`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Scenario> {
        Scenario::ALL.into_iter().find(|s| s.name() == name)
    }
}

/// A request the fake received, for tests to look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub query: String,
}

#[derive(Debug)]
struct Inner {
    scenario: Scenario,
    /// Scenarios that change over time read this; `next_day` moves it on.
    day: u32,
    seen: Vec<Seen>,
    /// Setup tokens minted and not yet claimed.
    tokens: BTreeSet<String>,
    /// `user:pass` logins the claims issued, and whether each still works.
    logins: BTreeMap<String, bool>,
    minted: u32,
    /// The currency every account reports (but the foreign one), `USD` unless set.
    currency: String,
    /// Dates count back from the real time instead of [`NOW`].
    system_clock: bool,
}

#[derive(Debug, Clone)]
struct Shared {
    inner: Arc<Mutex<Inner>>,
    /// The `host:port` the claim URLs and access URLs point at.
    host: Arc<str>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A poisoned lock only means a test thread panicked; the data is still fine.
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn record(&self, method: &Method, path: &str, query: Option<&str>) {
        self.lock().seen.push(Seen {
            method: method.to_string(),
            path: path.to_owned(),
            query: query.unwrap_or_default().to_owned(),
        });
    }
}

/// A running fake Bridge. Dropping it stops the server.
#[derive(Debug)]
pub struct FakeServer {
    shared: Shared,
    task: tokio::task::JoinHandle<()>,
    addr: SocketAddr,
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FakeServer {
    /// Starts on a free port of 127.0.0.1.
    ///
    /// # Errors
    /// If no port can be bound.
    pub async fn start(scenario: Scenario) -> std::io::Result<FakeServer> {
        FakeServer::start_on(SocketAddr::from(([127, 0, 0, 1], 0)), scenario).await
    }

    /// Starts on `listen`.
    ///
    /// # Errors
    /// If `listen` can't be bound.
    pub async fn start_on(listen: SocketAddr, scenario: Scenario) -> std::io::Result<FakeServer> {
        FakeServer::start_as(listen, None, scenario).await
    }

    /// Starts on `listen`, putting `public_host` (`host:port`) in the claim URLs and access URLs
    /// it hands out instead of its own address: the name the app reaches it by, as in a container
    /// network.
    ///
    /// # Errors
    /// If `listen` can't be bound.
    pub async fn start_as(
        listen: SocketAddr,
        public_host: Option<String>,
        scenario: Scenario,
    ) -> std::io::Result<FakeServer> {
        let listener = tokio::net::TcpListener::bind(listen).await?;
        let addr = listener.local_addr()?;
        let host = match public_host {
            Some(host) => host,
            None if addr.ip().is_unspecified() => format!("127.0.0.1:{}", addr.port()),
            None => addr.to_string(),
        };
        let shared = Shared {
            inner: Arc::new(Mutex::new(Inner {
                scenario,
                day: 0,
                seen: Vec::new(),
                tokens: BTreeSet::new(),
                logins: BTreeMap::new(),
                minted: 0,
                currency: "USD".to_owned(),
                system_clock: false,
            })),
            host: host.into(),
        };
        let app = Router::new()
            .route("/claim/{token}", post(claim))
            .route("/simplefin/accounts", get(accounts))
            .route("/_fake/next-day", post(next_day))
            .route("/_fake/scenario/{name}", post(set_scenario))
            .route("/_fake/token", post(mint_token))
            .route("/_fake/currency/{code}", post(set_currency))
            .with_state(shared.clone());
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(FakeServer { shared, task, addr })
    }

    /// The `host:port` to put in `BUDGET_SIMPLEFIN_CLAIM_HOSTS`.
    #[must_use]
    pub fn host(&self) -> String {
        self.shared.host.to_string()
    }

    /// The address it listens on.
    #[must_use]
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// A new setup token (base64 of a claim URL), good for one claim.
    #[must_use]
    pub fn setup_token(&self) -> String {
        STANDARD.encode(self.claim_url())
    }

    /// A new claim URL, good for one claim.
    #[must_use]
    pub fn claim_url(&self) -> String {
        mint(&self.shared)
    }

    /// Switches what the feed holds, and goes back to day 0.
    pub fn set_scenario(&self, scenario: Scenario) {
        let mut inner = self.shared.lock();
        inner.scenario = scenario;
        inner.day = 0;
    }

    /// Makes every account report `currency` (an ISO 4217 code), so the feed matches a
    /// household's; the foreign-currency scenario's extra account still differs.
    pub fn set_currency(&self, currency: &str) {
        currency.clone_into(&mut self.shared.lock().currency);
    }

    /// Counts every date back from the real time instead of the fixed [`NOW`], so an app on
    /// the real clock finds the rows inside its windows: the binary does this *(agent)*.
    pub fn follow_system_clock(&self) {
        self.shared.lock().system_clock = true;
    }

    /// Moves a day on, for the scenarios that change over time.
    pub fn next_day(&self) {
        self.shared.lock().day += 1;
    }

    /// Every request received so far.
    #[must_use]
    pub fn requests(&self) -> Vec<Seen> {
        self.shared.lock().seen.clone()
    }

    /// Makes the access URLs the claims issued stop working, as a revoked one does.
    pub fn revoke_logins(&self) {
        for working in self.shared.lock().logins.values_mut() {
            *working = false;
        }
    }
}

fn mint(shared: &Shared) -> String {
    let mut inner = shared.lock();
    inner.minted += 1;
    let token = format!("demo-token-{}", inner.minted);
    inner.tokens.insert(token.clone());
    format!("http://{}/claim/{token}", shared.host)
}

async fn mint_token(State(shared): State<Shared>) -> String {
    STANDARD.encode(mint(&shared))
}

async fn claim(State(shared): State<Shared>, Path(token): Path<String>) -> Response {
    shared.record(&Method::POST, &format!("/claim/{token}"), None);
    let mut inner = shared.lock();
    if !inner.tokens.remove(&token) {
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }
    let n = inner.logins.len() + 1;
    let login = format!("demo-user-{n}:demo-pass-{n}");
    inner.logins.insert(login.clone(), true);
    format!("http://{login}@{}/simplefin", shared.host).into_response()
}

async fn accounts(
    State(shared): State<Shared>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    shared.record(&Method::GET, "/simplefin/accounts", query.as_deref());
    let allowed = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Basic "))
        .and_then(|encoded| STANDARD.decode(encoded).ok())
        .and_then(|login| String::from_utf8(login).ok())
        .is_some_and(|login| shared.lock().logins.get(&login) == Some(&true));
    if !allowed {
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }
    let (scenario, day, currency, system_clock) = {
        let inner = shared.lock();
        (
            inner.scenario,
            inner.day,
            inner.currency.clone(),
            inner.system_clock,
        )
    };
    let now = if system_clock {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
            .unwrap_or(NOW)
    } else {
        NOW
    };
    match scenario {
        Scenario::Revoked => return (StatusCode::FORBIDDEN, "Forbidden").into_response(),
        Scenario::Unpaid => {
            return (StatusCode::PAYMENT_REQUIRED, "Payment Required").into_response();
        }
        _ => {}
    }
    let query = feed::Query::parse(query.as_deref().unwrap_or_default());
    let body = feed::response(scenario, day, &query, &currency, now);
    (
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

async fn next_day(State(shared): State<Shared>) -> StatusCode {
    shared.lock().day += 1;
    StatusCode::NO_CONTENT
}

async fn set_scenario(State(shared): State<Shared>, Path(name): Path<String>) -> StatusCode {
    let Some(scenario) = Scenario::parse(&name) else {
        return StatusCode::NOT_FOUND;
    };
    let mut inner = shared.lock();
    inner.scenario = scenario;
    inner.day = 0;
    StatusCode::NO_CONTENT
}

async fn set_currency(State(shared): State<Shared>, Path(code): Path<String>) -> StatusCode {
    if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_uppercase()) {
        return StatusCode::NOT_FOUND;
    }
    shared.lock().currency = code;
    StatusCode::NO_CONTENT
}
