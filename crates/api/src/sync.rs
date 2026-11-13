//! The sync engine (docs/spec/m2-bank-data.md#the-request and #sync-runs): one run asks the
//! Bridge for every account at once, reads each tracked account's rows, matches them to the
//! stored ones (`db::apply_synced_rows`) for every account that succeeded, and records the run
//! with a result per account. A failed or missing account keeps its last data and no miss counts
//! move. A newly tracked account is back-filled: 90 days as
//! two 45-day requests. Otherwise a run asks for the last 30 days.
//!
//! One run at a time: Sync now while a run is going joins it, and never makes a second request.
//! Every run asks the request budget first (`domain::sync_schedule`); Sync now also waits out a
//! cooldown after the last sync. When the run starts on its own is `schedule`'s.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use domain::feed::{self, SyncedRow, Window};
use domain::sync_health as rules_health;
use domain::sync_schedule::{self as rules, Asker, Spent};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use simplefin::{AccessUrl, Feed, FetchError};
use utoipa::ToSchema;

use crate::error::{Body, ErrorEnvelope};
use crate::{ApiError, AppState};

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/bank-connection/sync", post(sync_now))
        .route(
            "/bank-connection/sync-after-reconnect",
            post(sync_after_reconnect),
        )
        .route("/bank-connection/sync-settings", put(change_settings))
        .route("/bank-connection/sync-runs", get(history))
}

/// One sync run, as the screen shows it.
#[derive(Serialize, ToSchema)]
pub struct SyncRun {
    id: i64,
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
            id: run.id,
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
        (status = 409, description = "`not_connected`: no bank connection; `sync_paused`: the connection is paused until a new setup token is pasted; `sync_cooling_down`: a sync finished a moment ago; `sync_limit_reached`: the request budget is spent.", body = ErrorEnvelope),
    ),
)]
pub async fn sync_now(State(state): State<AppState>) -> Result<Json<SyncRun>, ApiError> {
    sync_on_request(&state, true).await
}

/// `POST /api/v1/bank-connection/sync-after-reconnect`: **Done, sync now**, once the household
/// has re-authorized a bank at SimpleFIN. Like Sync now, but it skips the cooldown; it still
/// counts against the request cap and joins a running sync.
#[utoipa::path(
    post,
    path = "/api/v1/bank-connection/sync-after-reconnect",
    operation_id = "syncAfterReconnect",
    responses(
        (status = 200, description = "The run, finished.", body = SyncRun),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 409, description = "`not_connected`: no bank connection; `sync_paused`: the connection is paused until a new setup token is pasted; `sync_limit_reached`: the request budget is spent.", body = ErrorEnvelope),
    ),
)]
pub async fn sync_after_reconnect(
    State(state): State<AppState>,
) -> Result<Json<SyncRun>, ApiError> {
    sync_on_request(&state, false).await
}

/// Runs a sync the household asked for, or joins the one running; `cooldown` makes it wait out
/// the cooldown after the last sync first.
async fn sync_on_request(state: &AppState, cooldown: bool) -> Result<Json<SyncRun>, ApiError> {
    match state.sync_lock.try_lock() {
        Ok(_running) => {
            let now = state.now();
            if cooldown {
                let last = db::last_finished_sync(&state.pool).await?;
                if let Some(until) = rules::cooldown_until(last, state.bank.sync_cooldown, now) {
                    return Err(cooling_down(last.unwrap_or(now), until, now));
                }
            }
            let ran = run(state, db::SyncTrigger::Manual).await?;
            crate::schedule::after_run(state, db::SyncTrigger::Manual, ran);
        }
        Err(_) => {
            // Join the run that's going: wait for it to end.
            drop(state.sync_lock.lock().await);
        }
    }
    let run = latest(state)
        .await?
        .ok_or_else(|| ApiError::internal(&"a sync ran, or was running, and left no run behind"))?;
    Ok(Json(run))
}

fn cooling_down(last: Timestamp, until: Timestamp, now: Timestamp) -> ApiError {
    let ago = now.duration_since(last).as_mins().max(0);
    let wait = (until.duration_since(now).as_secs().max(0) + 59) / 60;
    not_syncing(
        "sync_cooling_down",
        format!("Synced {ago} min ago. Sync now works again in {wait} min."),
    )
}

/// What the request budget allows now: the runs that spent requests in the last 24 hours.
pub(crate) async fn spent(state: &AppState, now: Timestamp) -> Result<Vec<Spent>, ApiError> {
    let runs = db::requests_since(&state.pool, now - rules::CAP_WINDOW).await?;
    Ok(runs
        .into_iter()
        .map(|(at, requests)| Spent {
            at,
            requests: u32::try_from(requests).unwrap_or(u32::MAX),
        })
        .collect())
}

/// "Sync limit reached, try again after 3:40 pm", in the household's timezone.
async fn limit_reached(state: &AppState, from: Option<Timestamp>) -> Result<ApiError, ApiError> {
    let when = match from {
        Some(at) => format!(", try again after {}", clock_time(state, at).await?),
        None => String::new(),
    };
    Ok(not_syncing(
        "sync_limit_reached",
        format!("Sync limit reached{when}."),
    ))
}

/// `at` as a time of day in the household's timezone, like `3:40 pm` *(agent: the server's
/// messages are English)*.
async fn clock_time(state: &AppState, at: Timestamp) -> Result<String, ApiError> {
    let household = db::household(&state.pool)
        .await?
        .ok_or_else(|| ApiError::internal(&"setup is done but there's no household"))?;
    let zoned = at
        .in_tz(household.timezone.name())
        .map_err(|error| ApiError::internal(&error))?;
    Ok(zoned.strftime("%-I:%M %P").to_string())
}

/// Back-fills the tracked accounts that haven't been, once any running sync has ended. Does
/// nothing when there's no connection, it is paused, or every account is back-filled.
pub(crate) async fn backfill(state: &AppState) -> Result<(), ApiError> {
    let _running = state.sync_lock.lock().await;
    match run(state, db::SyncTrigger::Backfill).await {
        Err(error) if error.is_conflict() => Ok(()),
        Err(error) => Err(error),
        Ok(ran) => {
            crate::schedule::after_run(state, db::SyncTrigger::Backfill, ran);
            Ok(())
        }
    }
}

fn not_syncing(code: &'static str, message: impl Into<String>) -> ApiError {
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

/// How a run went, for the schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ran {
    /// Nothing to do: a back-fill with every account back-filled.
    Nothing,
    Finished {
        /// The whole run failed.
        failed: bool,
        /// The run, or an account in it, failed in a way worth retrying.
        transient: bool,
    },
}

/// Refuses a run of `trigger` that would make `requests` requests past the budget.
async fn within_budget(
    state: &AppState,
    trigger: db::SyncTrigger,
    requests: usize,
    now: Timestamp,
) -> Result<(), ApiError> {
    let needed = u32::try_from(requests).unwrap_or(u32::MAX);
    let asker = if trigger == db::SyncTrigger::Scheduled {
        Asker::Scheduled
    } else {
        Asker::Other
    };
    let spent = spent(state, now).await?;
    let cap = state.bank.sync_request_cap;
    if rules::allows(&spent, now, cap, asker, needed) {
        return Ok(());
    }
    tracing::info!(
        trigger = trigger.as_str(),
        left = rules::requests_left(&spent, now, cap),
        "bank sync skipped: the request limit is reached"
    );
    let from = rules::allowed_from(&spent, now, cap, asker, needed);
    Err(limit_reached(state, from).await?)
}

/// Runs a sync, if the request budget allows it. The caller holds the lock.
pub(crate) async fn run(state: &AppState, trigger: db::SyncTrigger) -> Result<Ran, ApiError> {
    let (connection_id, access) = access_url(state).await?;
    let household = db::household(&state.pool)
        .await?
        .ok_or_else(|| ApiError::internal(&"setup is done but there's no household"))?;
    let links = db::tracked_feed_accounts(&state.pool, connection_id).await?;
    if trigger == db::SyncTrigger::Backfill && links.iter().all(|l| l.backfilled) {
        return Ok(Ran::Nothing);
    }
    let started = state.now();
    let requests = plan(&links, started.as_second());
    within_budget(state, trigger, requests.len(), started).await?;
    let keep_raw = match &state.bank.secret_key {
        Some(key) if db::sync_settings(&state.pool).await?.keep_raw_responses => Some(key.clone()),
        _ => None,
    };
    let run_id = db::start_sync_run(&state.pool, connection_id, trigger, started).await?;
    let mut end = db::RunEnd::default();
    // Each account's rows over every request, applied once the responses are in, so matching
    // and the rows the bank stopped sending see the run's whole response.
    let mut read: BTreeMap<i64, Read> = BTreeMap::new();
    let mut notices: Vec<String> = Vec::new();
    for (n, request) in requests.into_iter().enumerate() {
        end.requests += 1;
        let url = access.clone();
        let window = request.window;
        let fetched = tokio::task::spawn_blocking(move || simplefin::fetch_with_body(&url, window))
            .await
            .map_err(|error| ApiError::internal(&error))?;
        let feed = match fetched {
            Ok((feed, body)) => {
                if let Some(key) = &keep_raw {
                    keep_raw_response(state, key, run_id, n, &body).await?;
                }
                feed
            }
            Err(error) => {
                refused(state, &error, &mut end).await?;
                break;
            }
        };
        if let Some(revoked) = feed
            .problems
            .iter()
            .find(|p| rules_health::access_revoked(&p.code))
        {
            // A 200 that says the access URL was disabled pauses the connection like a 403.
            revoke(state, revoked, &mut end).await?;
            break;
        }
        notices.extend(
            feed.problems
                .iter()
                .filter(|p| p.bank_link.is_none() && p.account_id.is_none())
                .map(|p| p.code.clone()),
        );
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
    finish(
        state,
        trigger,
        connection_id,
        run_id,
        &links,
        &Outcome {
            end: &end,
            results: &results,
            notices: &notices,
        },
    )
    .await
}

/// What a run came to.
#[derive(Clone, Copy)]
struct Outcome<'a> {
    end: &'a db::RunEnd,
    results: &'a [db::AccountResult],
    /// The codes of the notices the feed carried with no bank link or account: logged, not shown.
    notices: &'a [String],
}

/// Records the run's end, marks what it back-filled, prunes the history and logs the run's line.
async fn finish(
    state: &AppState,
    trigger: db::SyncTrigger,
    connection_id: i64,
    run_id: i64,
    links: &[db::FeedLink],
    outcome: &Outcome<'_>,
) -> Result<Ran, ApiError> {
    let Outcome {
        end,
        results,
        notices,
    } = *outcome;
    let finished = state.now();
    db::finish_sync_run(&state.pool, run_id, end, results, finished).await?;
    if end.error_code.is_none() {
        let done: Vec<String> = links
            .iter()
            .filter(|l| !l.backfilled)
            .filter(|l| results.iter().any(|r| r.account_id == l.account_id && r.ok))
            .map(|l| l.external_id.clone())
            .collect();
        db::mark_backfilled(&state.pool, connection_id, &done, finished).await?;
    }
    db::prune_sync_history(
        &state.pool,
        finished,
        rules::RUNS_KEPT,
        rules::RAW_RESPONSES_KEPT,
    )
    .await?;
    let failed: Vec<&str> = results
        .iter()
        .filter_map(|r| r.error_code.as_deref())
        .collect();
    // The one log line per run: trigger, counts, requests and error codes, never amounts,
    // merchants or the access URL.
    tracing::info!(
        trigger = trigger.as_str(),
        requests = end.requests,
        added = end.added,
        updated = end.updated,
        error = end.error_code.as_deref().unwrap_or("none"),
        account_errors = ?failed,
        notices = ?notices,
        "bank sync ran"
    );
    let transient = end.error_code.as_deref().is_some_and(rules::transient)
        || failed.iter().any(|code| rules::transient(code));
    Ok(Ran::Finished {
        failed: end.error_code.is_some(),
        transient,
    })
}

/// Keeps response `n` of run `run_id`, sealed with the secret key.
async fn keep_raw_response(
    state: &AppState,
    key: &simplefin::SecretKey,
    run_id: i64,
    n: usize,
    body: &str,
) -> Result<(), ApiError> {
    let sealed = simplefin::seal_raw_response(key, body.as_bytes())
        .map_err(|error| ApiError::internal(&error))?;
    let seq = i64::try_from(n).unwrap_or(i64::MAX);
    db::keep_raw_response(&state.pool, run_id, seq, state.now(), &sealed).await?;
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

/// The feed says the access URL was revoked or disabled (`gen.auth`): the run fails and the
/// connection pauses, as for a 403.
async fn revoke(
    state: &AppState,
    problem: &simplefin::FeedProblem,
    end: &mut db::RunEnd,
) -> Result<(), ApiError> {
    db::set_connection_status(&state.pool, db::ConnectionStatus::TokenRevoked, state.now()).await?;
    end.error_code = Some(problem.code.clone());
    end.error = Some(feed::sanitize_message(&format!(
        "Sync failed: {}. The accounts keep their last data.",
        problem.message.trim_end_matches('.')
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
            bank_link: account.bank_link.clone(),
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

/// The daily schedule, the request budget and Sync now's wait, for the connection screen.
#[derive(Serialize, ToSchema)]
pub struct SyncState {
    /// The daily sync time, `HH:MM` in the household timezone.
    time: String,
    /// Raw bank responses are kept (sealed, 7 days) for troubleshooting.
    keep_raw_responses: bool,
    /// Bridge requests allowed in any rolling 24 hours.
    request_cap: u32,
    /// Of those, how many are left now.
    requests_left: u32,
    /// RFC 3339: when the last sync finished (back-fills aside); null before the first.
    #[schema(required = true)]
    last_synced_at: Option<String>,
    /// RFC 3339: Sync now waits until then after the last sync; null when it needn't.
    #[schema(required = true)]
    cooldown_until: Option<String>,
    /// RFC 3339: the request limit is reached until then; null when Sync now may ask.
    #[schema(required = true)]
    limit_until: Option<String>,
}

/// The sync time as set, or the default.
pub(crate) fn sync_time(settings: &db::SyncSettings) -> rules::SyncTime {
    settings
        .time
        .as_deref()
        .and_then(|time| rules::SyncTime::parse(time).ok())
        .unwrap_or_default()
}

pub(crate) async fn sync_state(state: &AppState) -> Result<SyncState, ApiError> {
    let now = state.now();
    let settings = db::sync_settings(&state.pool).await?;
    let spent = spent(state, now).await?;
    let last = db::last_finished_sync(&state.pool).await?;
    let cap = state.bank.sync_request_cap;
    let blocked = !rules::allows(&spent, now, cap, Asker::Other, 1);
    Ok(SyncState {
        time: sync_time(&settings).to_string(),
        keep_raw_responses: settings.keep_raw_responses,
        request_cap: cap,
        requests_left: rules::requests_left(&spent, now, cap),
        last_synced_at: last.map(|at| at.to_string()),
        cooldown_until: rules::cooldown_until(last, state.bank.sync_cooldown, now)
            .map(|at| at.to_string()),
        limit_until: blocked
            .then(|| rules::allowed_from(&spent, now, cap, Asker::Other, 1))
            .flatten()
            .map(|at| at.to_string()),
    })
}

/// The sync settings, changed.
#[derive(Deserialize, ToSchema)]
pub struct SyncSettingsChange {
    /// The daily sync time, `HH:MM` (24-hour) in the household timezone.
    sync_time: String,
    /// Keep raw bank responses, sealed, for 7 days. Turning it off deletes the kept ones.
    keep_raw_responses: bool,
}

/// `PUT /api/v1/bank-connection/sync-settings`: the daily sync time and whether raw bank
/// responses are kept. A time set to one already past today runs tomorrow.
#[utoipa::path(
    put,
    path = "/api/v1/bank-connection/sync-settings",
    operation_id = "changeSyncSettings",
    request_body = SyncSettingsChange,
    responses(
        (status = 200, description = "Changed; the connection screen now.", body = crate::bank::BankConnection),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 422, description = "`validation_failed` with `sync_time` if it isn't `HH:MM`.", body = ErrorEnvelope),
    ),
)]
pub async fn change_settings(
    State(state): State<AppState>,
    Body(body): Body<SyncSettingsChange>,
) -> Result<Json<crate::bank::BankConnection>, ApiError> {
    let time = rules::SyncTime::parse(&body.sync_time).map_err(|_| {
        ApiError::validation(
            "Check the sync time.",
            BTreeMap::from([(
                "sync_time".to_owned(),
                "Enter a time like 06:13.".to_owned(),
            )]),
        )
    })?;
    db::set_sync_settings(&state.pool, &time.to_string(), body.keep_raw_responses).await?;
    crate::schedule::time_changed(&state, time).await?;
    Ok(Json(crate::bank::current(&state).await?))
}

/// How one account fared in a run.
#[derive(Serialize, ToSchema)]
pub struct RunAccount {
    account_id: i64,
    /// The account's name in the app.
    name: String,
    ok: bool,
    /// Like `act.missingdata`, `con.auth` or `absent`; null when it went well.
    #[schema(required = true)]
    error_code: Option<String>,
    /// Sanitized, to show as is.
    #[schema(required = true)]
    error: Option<String>,
    /// RFC 3339: when the provider last refreshed the account's bank.
    #[schema(required = true)]
    balance_date: Option<String>,
}

/// A run in the history, with each account's result.
#[derive(Serialize, ToSchema)]
pub struct HistoryRun {
    run: SyncRun,
    /// By account name.
    accounts: Vec<RunAccount>,
}

/// The run history, newest first: 90 days of it.
#[derive(Serialize, ToSchema)]
pub struct SyncHistory {
    runs: Vec<HistoryRun>,
}

/// `GET /api/v1/bank-connection/sync-runs`: Recent syncs.
#[utoipa::path(
    get,
    path = "/api/v1/bank-connection/sync-runs",
    operation_id = "listSyncRuns",
    responses(
        (status = 200, description = "The runs, newest first.", body = SyncHistory),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
    ),
)]
pub async fn history(State(state): State<AppState>) -> Result<Json<SyncHistory>, ApiError> {
    let runs = db::sync_history(&state.pool).await?;
    Ok(Json(SyncHistory {
        runs: runs
            .into_iter()
            .map(|entry| HistoryRun {
                run: entry.run.into(),
                accounts: entry
                    .accounts
                    .into_iter()
                    .map(|a| RunAccount {
                        account_id: a.account_id,
                        name: a.name,
                        ok: a.ok,
                        error_code: a.error_code,
                        error: a.error,
                        balance_date: a.balance_date.map(|at| at.to_string()),
                    })
                    .collect(),
            })
            .collect(),
    }))
}

/// The cooldown Sync now waits out by default.
pub(crate) const DEFAULT_COOLDOWN: SignedDuration = rules::COOLDOWN;
