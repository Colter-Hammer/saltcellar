//! The HTTP API: JSON under `/api/v1/…` (docs/spec/foundations.md#http-api).

mod accounts;
mod auth;
mod bank;
mod batches;
mod categories;
mod connection_accounts;
mod error;
mod health;
mod household;
mod import;
mod manual;
mod middleware;
mod profile;
mod reset;
mod schedule;
mod session;
mod setup;
mod sync;
mod sync_flags;
mod sync_health;
#[cfg(test)]
mod tests;
mod transactions;
mod undecided;

use std::path::Path;
use std::sync::Arc;

use axum::routing::{any, delete, get, patch, post, put};
use axum::{Json, Router};
use domain::SetupCode;
use jiff::Timestamp;
use utoipa::OpenApi;

pub use error::ApiError;
pub use reset::{IssueResetError, ResetLink, issue_password_reset};
pub use simplefin::{ClaimPolicy, SecretKey};

#[derive(OpenApi)]
#[openapi(
    info(title = "Saltcellar", description = "The Saltcellar HTTP API."),
    paths(
        health::get,
        setup::status,
        setup::currencies,
        setup::check_code,
        setup::finish,
        session::get,
        session::log_in,
        session::log_out,
        reset::check,
        reset::use_reset,
        profile::get,
        profile::change,
        profile::change_palette,
        profile::change_password,
        profile::log_out_everywhere,
        household::get,
        accounts::list,
        accounts::get,
        accounts::create,
        accounts::update_balance,
        accounts::change,
        accounts::order,
        bank::get,
        bank::claim,
        bank::disconnect,
        sync::sync_now,
        sync::sync_after_reconnect,
        sync::change_settings,
        sync::history,
        connection_accounts::list,
        connection_accounts::track,
        connection_accounts::ignore,
        transactions::list,
        transactions::get,
        transactions::tags,
        transactions::set_excluded,
        manual::create,
        manual::change,
        manual::delete,
        manual::payees,
        sync_flags::resolve,
        sync_flags::set_splits,
        sync_flags::dropped,
        sync_flags::restore,
        undecided::list,
        undecided::move_group,
        undecided::set_category,
        categories::list,
        categories::use_starter,
        categories::order,
        categories::add_group,
        categories::change_group,
        categories::delete_group,
        categories::add_category,
        categories::change_category,
        categories::delete_category,
        import::upload,
        import::get,
        import::commit,
        batches::list,
        batches::undo
    ),
    components(schemas(error::ErrorEnvelope))
)]
struct Doc;

/// The OpenAPI document for the whole API, generated from the routes and types below.
/// `just gen-api` writes it to `web/src/api/openapi.json`, where the frontend's types come from.
#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    Doc::openapi()
}

/// What the bank connection needs from the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BankConfig {
    /// `BUDGET_SECRET_KEY`, if set. Without it no setup token is claimed.
    pub secret_key: Option<SecretKey>,
    /// Where claim URLs may point: any `https://` one unless the dev setting narrows it.
    pub claim_policy: ClaimPolicy,
    /// How long Sync now waits after a sync: 30 minutes unless the dev setting changes it.
    pub sync_cooldown: jiff::SignedDuration,
    /// Bridge requests allowed in any rolling 24 hours: 12 unless the dev setting changes it.
    pub sync_request_cap: u32,
}

impl Default for BankConfig {
    fn default() -> Self {
        BankConfig {
            secret_key: None,
            claim_policy: ClaimPolicy::default(),
            sync_cooldown: sync::DEFAULT_COOLDOWN,
            sync_request_cap: domain::sync_schedule::REQUEST_CAP,
        }
    }
}

/// What every handler can reach.
#[derive(Clone)]
pub struct AppState {
    pub pool: db::SqlitePool,
    bank: Arc<BankConfig>,
    setup: Arc<setup::Setup>,
    uploads: Arc<import::Uploads>,
    clock: Arc<dyn Fn() -> Timestamp + Send + Sync>,
    /// Held while a bank sync runs: one at a time.
    sync_lock: Arc<tokio::sync::Mutex<()>>,
    /// The retry and catch-up waiting to run, and the scheduled slot already handled.
    schedule: Arc<std::sync::Mutex<schedule::Pending>>,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error(transparent)]
    Db(#[from] db::Error),
    #[error("can't make the setup code: no random bytes from the OS: {0}")]
    Random(getrandom::Error),
}

impl AppState {
    /// The state for a migrated database, with the data dir that holds it, where uploads wait too.
    /// While setup isn't finished, this start gets a new setup code: see [`AppState::setup_code`].
    ///
    /// # Errors
    /// If the database fails, or the OS has no random bytes for the code.
    pub async fn load(pool: db::SqlitePool, data_dir: &Path) -> Result<AppState, LoadError> {
        let setup = if db::household(&pool).await?.is_some() {
            setup::Setup::finished()
        } else {
            let mut bytes = [0u8; SetupCode::LEN];
            getrandom::fill(&mut bytes).map_err(LoadError::Random)?;
            setup::Setup::pending(SetupCode::from_random(bytes))
        };
        Ok(AppState {
            pool,
            bank: Arc::new(BankConfig::default()),
            setup: Arc::new(setup),
            uploads: Arc::new(import::Uploads::new(data_dir)),
            clock: Arc::new(Timestamp::now),
            sync_lock: Arc::new(tokio::sync::Mutex::new(())),
            schedule: Arc::default(),
        })
    }

    /// The same state with the bank connection's environment. A connection whose secret
    /// `bank.secret_key` can't open is marked `key_mismatch` (shown as paused; the app still
    /// starts); one marked so whose secret opens again goes back to `ok`.
    ///
    /// # Errors
    /// If the database fails.
    pub async fn with_bank(self, bank: BankConfig) -> Result<AppState, LoadError> {
        let state = AppState {
            bank: Arc::new(bank),
            ..self
        };
        state.check_connection_key().await?;
        Ok(state)
    }

    async fn check_connection_key(&self) -> Result<(), db::Error> {
        let Some((_, sealed)) = db::connection_secret(&self.pool).await? else {
            return Ok(());
        };
        let opens = self
            .bank
            .secret_key
            .as_ref()
            .is_some_and(|key| simplefin::open(key, &sealed).is_ok());
        let marked = db::connection(&self.pool)
            .await?
            .is_some_and(|c| c.status == db::ConnectionStatus::KeyMismatch);
        if !opens {
            tracing::warn!(
                "BUDGET_SECRET_KEY can't open the bank connection's secret; sync is paused"
            );
            db::set_connection_status(&self.pool, db::ConnectionStatus::KeyMismatch, self.now())
                .await?;
        } else if marked {
            db::set_connection_status(&self.pool, db::ConnectionStatus::Ok, self.now()).await?;
        }
        Ok(())
    }

    /// The same state, telling the time by `clock`, so tests can move it on.
    #[cfg(test)]
    pub(crate) fn with_clock(self, clock: impl Fn() -> Timestamp + Send + Sync + 'static) -> Self {
        AppState {
            clock: Arc::new(clock),
            ..self
        }
    }

    /// The time now, by the state's clock.
    fn now(&self) -> Timestamp {
        (self.clock)()
    }

    /// Today in the household's timezone, by the state's clock.
    async fn today(&self) -> Result<domain::Date, ApiError> {
        // Behind the setup gate, so the household exists unless the database lost it.
        let household = db::household(&self.pool)
            .await?
            .ok_or_else(|| ApiError::internal(&"setup is done but there's no household"))?;
        let today = self
            .now()
            .in_tz(household.timezone.name())
            .map_err(|error| ApiError::internal(&error))?
            .date();
        domain::Date::from_ymd(today.year(), today.month(), today.day())
            .map_err(|error| ApiError::internal(&error))
    }

    /// The code that claims the install, until setup is finished. The caller prints it to the log,
    /// the one secret ever logged on purpose.
    #[must_use]
    pub fn setup_code(&self) -> Option<&SetupCode> {
        self.setup.code()
    }
}

/// The whole API, with the shared rules applied to every route:
/// one log line per request, and mutating requests must be JSON.
/// Until setup is finished, everything but health and the setup routes answers `setup_required`;
/// after, everything but those, logging in and the reset link answers `login_required` without a
/// live session.
/// Every path under `/api` is the API's, so an unknown one gets the JSON 404, never the frontend.
pub fn router(state: AppState) -> Router {
    let open = Router::new()
        .route("/health", get(health::get))
        .route("/setup", get(setup::status).post(setup::finish))
        .route("/setup/currencies", get(setup::currencies))
        .route("/setup/code", post(setup::check_code));
    let logged_in = Router::new()
        .route("/session", get(session::get).delete(session::log_out))
        .route("/sessions", delete(profile::log_out_everywhere))
        .route("/profile", get(profile::get).put(profile::change))
        .route("/profile/palette", put(profile::change_palette))
        .route("/profile/password", post(profile::change_password))
        .route("/household", get(household::get))
        .route("/accounts", get(accounts::list).post(accounts::create))
        .route("/accounts/order", put(accounts::order))
        .route("/accounts/{id}", get(accounts::get).patch(accounts::change))
        .route("/accounts/{id}/balance", post(accounts::update_balance))
        .route(
            "/bank-connection",
            get(bank::get).put(bank::claim).delete(bank::disconnect),
        )
        .merge(sync::routes())
        .route("/connection-accounts", get(connection_accounts::list))
        .route(
            "/connection-accounts/{external_account_id}/track",
            post(connection_accounts::track),
        )
        .route(
            "/connection-accounts/{external_account_id}/ignore",
            post(connection_accounts::ignore),
        )
        .route(
            "/transactions",
            get(transactions::list).post(manual::create),
        )
        .route(
            "/transactions/{id}",
            get(transactions::get)
                .put(manual::change)
                .delete(manual::delete),
        )
        .route("/transactions/{id}/category", put(undecided::set_category))
        .merge(sync_flags::routes())
        .route(
            "/transactions/{id}/excluded",
            put(transactions::set_excluded),
        )
        .route("/undecided", get(undecided::list))
        .route("/undecided/move", post(undecided::move_group))
        .route("/tags", get(transactions::tags))
        .route("/payees", get(manual::payees))
        .route(
            "/categories",
            get(categories::list).post(categories::add_category),
        )
        .route("/categories/starter", post(categories::use_starter))
        .route("/categories/order", put(categories::order))
        .route(
            "/categories/{id}",
            patch(categories::change_category).delete(categories::delete_category),
        )
        .route("/category-groups", post(categories::add_group))
        .route("/imports", get(batches::list))
        .route("/imports/{id}/undo", post(batches::undo))
        .route("/imports/actual", post(import::upload))
        .route("/imports/actual/{id}", get(import::get))
        .route("/imports/actual/{id}/commit", post(import::commit))
        .route(
            "/category-groups/{id}",
            patch(categories::change_group).delete(categories::delete_group),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            session::require_login,
        ));
    let gated = Router::new()
        .route("/session", post(session::log_in))
        .route("/password-reset", post(reset::use_reset))
        .route("/password-reset/check", post(reset::check))
        .merge(logged_in)
        .method_not_allowed_fallback(error::method_not_allowed)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            setup::gate,
        ));
    let v1 = open
        .merge(gated)
        .method_not_allowed_fallback(error::method_not_allowed);
    Router::new()
        .nest("/api/v1", v1)
        .route("/api/v1/", any(setup::not_found))
        .route("/api/v1/{*rest}", any(setup::not_found))
        .route("/api/openapi.json", get(Json(openapi())))
        .route("/api", any(error::not_found))
        .route("/api/", any(error::not_found))
        .route("/api/{*rest}", any(error::not_found))
        .layer(axum::middleware::from_fn(middleware::require_json))
        .layer(axum::middleware::from_fn(middleware::log_request))
        .with_state(state)
}
