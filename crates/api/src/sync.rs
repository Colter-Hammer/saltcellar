//! The sync engine (docs/spec/m2-bank-data.md#the-request and #sync-runs): one run asks the
//! Bridge for every account at once, reads each tracked account's rows, matches them to the
//! stored ones (`db::apply_synced_rows`) for every account that succeeded, and records the run
//! with a result per account. A failed or missing account keeps its last data and no miss counts
//! move. A newly tracked account is back-filled: 90 days as
//! two 45-day requests. Otherwise a run asks for the last 30 days.
//!
//! One run at a time: Sync now while a run is going joins it, and never makes a second request.
//! The cooldown and the request cap arrive with the schedule.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use domain::feed::{self, SyncedRow, Window};
use jiff::Timestamp;
use serde::Serialize;
use simplefin::{AccessUrl, Feed, FetchError};
use utoipa::ToSchema;

use crate::error::ErrorEnvelope;
use crate::{ApiError, AppState};

/// One sync run, as the screen shows it.
#[derive(Serialize, ToSchema)]
pub struct SyncRun {
    /// What started it: `manual` (Sync now), `backfill` (tracking an account), `scheduled`,
    /// `retry` or `catch_up`.
    trigger: String,
    /// RFC 3339.
    started_at: String,
    /// Null while it runs.
    #[schema(required = true)]
    finished_at: Option<String>,
    /// Bridge requests it made.
    requests: i64,
    /// Rows it added.
    added: i64,
    /// Rows whose amount, date, status or bank text it changed.
    updated: i64,
    /// Why the whole run failed, to show as is; null when it didn't.
    #[schema(required = true)]
    error: Option<String>,
}

impl From<db::SyncRun> for SyncRun {
    fn from(run: db::SyncRun) -> Self {
        SyncRun {
            trigger: run.trigger.as_str().to_owned(),
            started_at: run.started_at.to_string(),
            finished_at: run.finished_at.map(|at| at.to_string()),
            requests: run.requests,
            added: run.added,
            updated: run.updated,
            error: run.error,
        }
    }
}

/// The newest run, for the bank connection screen.
pub(crate) async fn latest(state: &AppState) -> Result<Option<SyncRun>, ApiError> {
    Ok(db::latest_sync_run(&state.pool).await?.map(SyncRun::from))
}

/// `POST /api/v1/bank-connection/sync`: Sync now. Runs a sync and answers how it went; while one
/// is running, waits for it and answers that one instead of asking the Bridge again. A run the
/// Bridge failed still answers 200, with the run's error.
#[utoipa::path(
    post,
    path = "/api/v1/bank-connection/sync",
    operation_id = "syncNow",
    responses(
        (status = 200, description = "The run, finished.", body = SyncRun),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 409, description = "`not_connected`: no bank connection; `sync_paused`: the connection is paused until a new setup token is pasted.", body = ErrorEnvelope),
    ),
)]
pub async fn sync_now(State(state): State<AppState>) -> Result<Json<SyncRun>, ApiError> {
    match state.sync_lock.try_lock() {
        Ok(_running) => run(&state, db::SyncTrigger::Manual).await?,
        Err(_) => {
            // Join the run that's going: wait for it to end.
            drop(state.sync_lock.lock().await);
        }
    }
    let run = latest(&state)
        .await?
        .ok_or_else(|| ApiError::internal(&"a sync ran, or was running, and left no run behind"))?;
    Ok(Json(run))
}

/// Back-fills the tracked accounts that haven't been, once any running sync has ended. Does
/// nothing when there's no connection, it is paused, or every account is back-filled.
pub(crate) async fn backfill(state: &AppState) -> Result<(), ApiError> {
    let _running = state.sync_lock.lock().await;
    match run(state, db::SyncTrigger::Backfill).await {
        Err(error) if error.is_conflict() => Ok(()),
        other => other,
    }
}

fn not_syncing(code: &'static str, message: &'static str) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, code, message)
}

/// The access URL, opened with the secret key. A key that can't open it pauses the connection.
async fn access_url(state: &AppState) -> Result<(i64, AccessUrl), ApiError> {
    let paused = || {
        not_syncing(
            "sync_paused",
            "Bank sync is paused. Paste a new setup token in Settings › Bank connection & sync.",
        )
    };
    let Some(connection) = db::connection(&state.pool).await? else {
        return Err(not_syncing(
            "not_connected",
            "There's no bank connection. Paste a setup token first.",
        ));
    };
    if connection.status != db::ConnectionStatus::Ok {
        return Err(paused());
    }
    let sealed = db::connection_secret(&state.pool).await?;
    let opened = match (&state.bank.secret_key, sealed) {
        (Some(key), Some((_, sealed))) => simplefin::open(key, &sealed).ok(),
        _ => None,
    };
    let Some(access) = opened else {
        tracing::warn!("BUDGET_SECRET_KEY can't open the bank connection's secret; sync is paused");
        db::set_connection_status(&state.pool, db::ConnectionStatus::KeyMismatch, state.now())
            .await?;
        return Err(paused());
    };
    Ok((connection.id, access))
}

/// One request and the tracked accounts whose rows it brings.
struct Request<'a> {
    window: Window,
    accounts: Vec<&'a db::FeedLink>,
}

/// The requests a run makes: the back-fill's two when any tracked account still needs it (the
/// newer half serves every account, the older only those), else the daily window.
fn plan(links: &[db::FeedLink], now: i64) -> Vec<Request<'_>> {
    let backfilling: Vec<&db::FeedLink> = links.iter().filter(|l| !l.backfilled).collect();
    if backfilling.is_empty() {
        return vec![Request {
            window: feed::daily_window(now),
            accounts: links.iter().collect(),
        }];
    }
    let [newer, older] = feed::backfill_windows(now);
    vec![
        Request {
            window: newer,
            accounts: links.iter().collect(),
        },
        Request {
            window: older,
            accounts: backfilling,
        },
    ]
}

/// Runs a sync. The caller holds the lock.
async fn run(state: &AppState, trigger: db::SyncTrigger) -> Result<(), ApiError> {
    let (connection_id, access) = access_url(state).await?;
    let household = db::household(&state.pool)
        .await?
        .ok_or_else(|| ApiError::internal(&"setup is done but there's no household"))?;
    let links = db::tracked_feed_accounts(&state.pool, connection_id).await?;
    if trigger == db::SyncTrigger::Backfill && links.iter().all(|l| l.backfilled) {
        return Ok(());
    }
    let started = state.now();
    let run_id = db::start_sync_run(&state.pool, connection_id, trigger, started).await?;
    let mut end = db::RunEnd::default();
    // Each account's rows over every request, applied once the responses are in, so matching
    // and the rows the bank stopped sending see the run's whole response.
    let mut read: BTreeMap<i64, Read> = BTreeMap::new();
    for (n, request) in plan(&links, started.as_second()).into_iter().enumerate() {
        end.requests += 1;
        let url = access.clone();
        let window = request.window;
        let fetched = tokio::task::spawn_blocking(move || simplefin::fetch(&url, window))
            .await
            .map_err(|error| ApiError::internal(&error))?;
        let feed = match fetched {
            Ok(feed) => feed,
            Err(error) => {
                refused(state, &error, &mut end).await?;
                break;
            }
        };
        if n == 0 {
            // Every response refreshes the new accounts prompt.
            record_accounts(state, connection_id, &feed, household.currency.code()).await?;
        }
        for link in request.accounts {
            let account = read_account(link, &feed, household.currency, window.start);
            match read.entry(link.account_id) {
                Entry::Vacant(entry) => {
                    entry.insert(account);
                }
                // A failure in either request stands for the run.
                Entry::Occupied(mut entry) => entry.get_mut().join(account),
            }
        }
    }
    for (account_id, account) in &read {
        let (true, Some(rows)) = (account.result.ok, &account.rows) else {
            continue;
        };
        let window_start = feed::utc_date(account.window_start)
            .ok_or_else(|| ApiError::internal(&"a sync window starts off the calendar"))?;
        let applied =
            db::apply_synced_rows(&state.pool, *account_id, rows, window_start, state.now())
                .await?;
        end.added += applied.added;
        end.updated += applied.updated;
    }
    let results: Vec<db::AccountResult> = read.into_values().map(|a| a.result).collect();
    let finished = state.now();
    db::finish_sync_run(&state.pool, run_id, &end, &results, finished).await?;
    if end.error_code.is_none() {
        let done: Vec<String> = links
            .iter()
            .filter(|l| !l.backfilled)
            .filter(|l| results.iter().any(|r| r.account_id == l.account_id && r.ok))
            .map(|l| l.external_id.clone())
            .collect();
        db::mark_backfilled(&state.pool, connection_id, &done, finished).await?;
    }
    let failed: Vec<&str> = results
        .iter()
        .filter_map(|r| r.error_code.as_deref())
        .collect();
    tracing::info!(
        trigger = trigger.as_str(),
        requests = end.requests,
        added = end.added,
        updated = end.updated,
        error = end.error_code.as_deref().unwrap_or("none"),
        account_errors = ?failed,
        "bank sync ran"
    );
    Ok(())
}

/// Records why the Bridge refused the run; a revoked access URL or an unpaid subscription pauses
/// the connection.
async fn refused(
    state: &AppState,
    error: &FetchError,
    end: &mut db::RunEnd,
) -> Result<(), ApiError> {
    let (code, pause) = match error {
        FetchError::Unreachable => ("unreachable".to_owned(), None),
        FetchError::Revoked => ("403".to_owned(), Some(db::ConnectionStatus::TokenRevoked)),
        FetchError::Unpaid => ("402".to_owned(), Some(db::ConnectionStatus::Unpaid)),
        FetchError::Refused(status) => (status.to_string(), None),
        FetchError::BadAnswer => ("bad_answer".to_owned(), None),
    };
    if let Some(status) = pause {
        db::set_connection_status(&state.pool, status, state.now()).await?;
    }
    end.error_code = Some(code);
    end.error = Some(feed::sanitize_message(&format!(
        "Sync failed: {error}. The accounts keep their last data."
    )));
    Ok(())
}

async fn record_accounts(
    state: &AppState,
    connection_id: i64,
    feed: &Feed,
    currency: &str,
) -> Result<(), ApiError> {
    let reported: Vec<db::ReportedAccount> = feed
        .accounts
        .iter()
        .map(|account| db::ReportedAccount {
            external_id: account.id.clone(),
            name: account.name.clone(),
            institution: account.institution.clone(),
            currency: account.currency.clone(),
        })
        .collect();
    db::record_connection_accounts(&state.pool, connection_id, &reported, currency, state.now())
        .await?;
    Ok(())
}

/// What a run read for one account.
struct Read {
    result: db::AccountResult,
    /// Its rows; `None` for a balances-only account, whose rows are ignored.
    rows: Option<Vec<SyncedRow>>,
    /// Epoch seconds: where the earliest request that brought its rows started.
    window_start: i64,
}

impl Read {
    /// Adds what another request of the same run read for the account. A failure stands.
    fn join(&mut self, other: Read) {
        if !self.result.ok {
            return;
        }
        if !other.result.ok {
            *self = other;
            return;
        }
        if let (Some(rows), Some(more)) = (&mut self.rows, other.rows) {
            rows.extend(more);
        }
        self.window_start = self.window_start.min(other.window_start);
    }
}

/// Reads one tracked account out of the feed of a request whose window started at
/// `window_start`.
fn read_account(
    link: &db::FeedLink,
    feed: &Feed,
    currency: domain::Currency,
    window_start: i64,
) -> Read {
    let failed = |code: &str, message: String, balance_date| Read {
        result: db::AccountResult {
            account_id: link.account_id,
            ok: false,
            error_code: Some(code.to_owned()),
            error: Some(feed::sanitize_message(&message)),
            balance_date,
        },
        rows: None,
        window_start,
    };
    let Some(account) = feed.accounts.iter().find(|a| a.id == link.external_id) else {
        return failed(
            "absent",
            "The bank didn't send this account this time.".to_owned(),
            None,
        );
    };
    let balance_date = account
        .balance_date
        .and_then(|at| Timestamp::from_second(at).ok());
    // An error naming the account, or its bank link without naming another account.
    let problem = feed.problems.iter().find(|p| {
        p.account_id.as_deref() == Some(account.id.as_str())
            || (p.account_id.is_none() && p.bank_link.is_some() && p.bank_link == account.bank_link)
    });
    if let Some(problem) = problem {
        return failed(&problem.code, problem.message.clone(), balance_date);
    }
    let rows = if link.balances_only {
        None
    } else {
        match feed::read_rows(&account.rows, currency) {
            Ok(rows) => Some(rows),
            Err(error) => return failed(error.code(), error.to_string(), balance_date),
        }
    };
    Read {
        result: db::AccountResult {
            account_id: link.account_id,
            ok: true,
            error_code: None,
            error: None,
            balance_date,
        },
        rows,
        window_start,
    }
}
