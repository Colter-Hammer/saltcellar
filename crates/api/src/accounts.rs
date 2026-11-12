//! Accounts (docs/spec/m1-ledger-import.md#accounts): the Actual importer creates them in
//! milestone 1; milestone 2 edits them and adds manual accounts with their manual balance
//! snapshots (docs/spec/m2-bank-data.md#accounts).

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{Body, ErrorEnvelope, Id};
use crate::{ApiError, AppState};

/// What kind of account it is.
#[derive(Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccountType {
    Checking,
    Savings,
    CreditCard,
    Investment,
    Loan,
    Cash,
    Other,
}

impl From<domain::accounts::AccountType> for AccountType {
    fn from(kind: domain::accounts::AccountType) -> Self {
        use domain::accounts::AccountType as Type;
        match kind {
            Type::Checking => AccountType::Checking,
            Type::Savings => AccountType::Savings,
            Type::CreditCard => AccountType::CreditCard,
            Type::Investment => AccountType::Investment,
            Type::Loan => AccountType::Loan,
            Type::Cash => AccountType::Cash,
            Type::Other => AccountType::Other,
        }
    }
}

impl From<AccountType> for domain::accounts::AccountType {
    fn from(kind: AccountType) -> Self {
        use domain::accounts::AccountType as Type;
        match kind {
            AccountType::Checking => Type::Checking,
            AccountType::Savings => Type::Savings,
            AccountType::CreditCard => Type::CreditCard,
            AccountType::Investment => Type::Investment,
            AccountType::Loan => Type::Loan,
            AccountType::Cash => Type::Cash,
            AccountType::Other => Type::Other,
        }
    }
}

/// Whether an account's activity counts toward budgets, or only its balance over time is shown.
#[derive(Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum AccountRole {
    Budgeted,
    Tracking,
}

impl From<AccountRole> for domain::accounts::Role {
    fn from(role: AccountRole) -> Self {
        match role {
            AccountRole::Budgeted => domain::accounts::Role::Budgeted,
            AccountRole::Tracking => domain::accounts::Role::Tracking,
        }
    }
}

impl From<domain::accounts::Role> for AccountRole {
    fn from(role: domain::accounts::Role) -> Self {
        match role {
            domain::accounts::Role::Budgeted => AccountRole::Budgeted,
            domain::accounts::Role::Tracking => AccountRole::Tracking,
        }
    }
}

/// An account, its figures and its balances.
#[derive(Serialize, ToSchema)]
pub struct Account {
    id: i64,
    name: String,
    /// Null when none is known.
    #[schema(required = true)]
    institution: Option<String>,
    #[serde(rename = "type")]
    kind: AccountType,
    role: AccountRole,
    closed: bool,
    /// Whether it counts in net worth.
    in_net_worth: bool,
    /// Balances only: it holds balance snapshots and no transactions.
    balances_only: bool,
    /// Its connection; null for an account with none. One with a connection gets its balance from
    /// the bank and takes no manual balance, unless `balance_entry` says so.
    #[schema(required = true)]
    connection_id: Option<i64>,
    /// The balance shown, its latest balance snapshot, in minor units; null before it has one.
    #[schema(required = true)]
    balance_minor: Option<i64>,
    /// The app's own balance, its opening balance plus its posted transactions, in minor units;
    /// null without an opening balance. Where it differs from `balance_minor`, that's drift.
    #[schema(required = true)]
    app_balance_minor: Option<i64>,
    /// In minor units; null for a balances-only account.
    #[schema(required = true)]
    opening_balance_minor: Option<i64>,
    /// `YYYY-MM-DD`; null for a balances-only account.
    #[schema(required = true)]
    opening_date: Option<String>,
    /// How many transactions it has.
    rows: u64,
    /// `YYYY-MM-DD`, the date of its latest transaction; null without one.
    #[schema(required = true)]
    last_transaction_date: Option<String>,
    /// How its bank data stands; null for an account that isn't synced.
    #[schema(required = true)]
    sync: Option<crate::sync_health::AccountSync>,
    /// Who enters its balance.
    balance_entry: BalanceEntry,
}

/// Who enters an account's balance.
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BalanceEntry {
    /// The household does (Update balance): an account with no connection, or a balances-only
    /// one whose bank link is broken or whose connection is paused.
    Manual,
    /// The bank sends it.
    Bank,
}

impl From<db::AccountBalance> for Account {
    fn from(account: db::AccountBalance) -> Self {
        Account {
            id: account.id,
            name: account.name,
            institution: account.institution,
            kind: account.kind.into(),
            role: account.role.into(),
            closed: account.closed,
            in_net_worth: account.in_net_worth,
            balances_only: !account.has_transactions,
            connection_id: account.connection_id,
            balance_minor: account.snapshot.map(domain::Money::minor),
            app_balance_minor: account.app_balance.map(domain::Money::minor),
            opening_balance_minor: account.opening.map(|opening| opening.balance.minor()),
            opening_date: account.opening.map(|opening| opening.date.to_string()),
            rows: account.rows,
            last_transaction_date: account.last_transaction.map(|date| date.to_string()),
            balance_entry: if account.connection_id.is_none() {
                BalanceEntry::Manual
            } else {
                BalanceEntry::Bank
            },
            sync: None,
        }
    }
}

/// `account` with its bank data's health, and the manual balance it takes because of it.
fn with_sync(account: db::AccountBalance, health: &crate::sync_health::Snapshot) -> Account {
    let id = account.id;
    let mut account = Account::from(account);
    account.sync = health.account(id);
    if health.takes_manual_balance(id) {
        account.balance_entry = BalanceEntry::Manual;
    }
    account
}

#[derive(Serialize, ToSchema)]
pub struct AccountList {
    /// In order, closed ones included.
    accounts: Vec<Account>,
}

/// `GET /api/v1/accounts`: every account in order, with its balances.
#[utoipa::path(
    get,
    path = "/api/v1/accounts",
    operation_id = "getAccounts",
    responses(
        (status = 200, description = "Every account, closed ones included.", body = AccountList),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
    ),
)]
pub async fn list(State(state): State<AppState>) -> Result<Json<AccountList>, ApiError> {
    let accounts = db::account_balances(&state.pool).await?;
    let health = crate::sync_health::Snapshot::load(&state).await?;
    Ok(Json(AccountList {
        accounts: accounts
            .into_iter()
            .map(|account| with_sync(account, &health))
            .collect(),
    }))
}

/// `GET /api/v1/accounts/{id}`: one account, with its balances.
#[utoipa::path(
    get,
    path = "/api/v1/accounts/{id}",
    operation_id = "getAccount",
    params(("id" = i64, Path, description = "The account.")),
    responses(
        (status = 200, description = "The account.", body = Account),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such account.", body = ErrorEnvelope),
    ),
)]
pub async fn get(State(state): State<AppState>, Id(id): Id) -> Result<Json<Account>, ApiError> {
    let account = db::account_balance(&state.pool, id).await?.ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "There's no such account.",
        )
    })?;
    let health = crate::sync_health::Snapshot::load(&state).await?;
    Ok(Json(with_sync(account, &health)))
}

/// A manual account: one with no connection, whose balance is entered by hand.
#[derive(Deserialize, ToSchema)]
pub struct NewAccount {
    /// Unique among the accounts, ignoring case.
    name: String,
    #[serde(rename = "type")]
    kind: AccountType,
    role: AccountRole,
    /// What's in it today, in minor units: its first balance, and its opening balance, dated the
    /// day before *(agent)*.
    balance_minor: i64,
}

/// `POST /api/v1/accounts`: creates a manual account, last in the order.
#[utoipa::path(
    post,
    path = "/api/v1/accounts",
    operation_id = "addAccount",
    request_body = NewAccount,
    responses(
        (status = 201, description = "The account is created.", body = Account),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 422, description = "`validation_failed`: the name is empty, too long or taken by another account (any case).", body = ErrorEnvelope),
    ),
)]
pub async fn create(
    State(state): State<AppState>,
    Body(body): Body<NewAccount>,
) -> Result<(StatusCode, Json<Account>), ApiError> {
    let name = valid_name(&body.name)?;
    let new = db::NewAccount {
        name: name.clone(),
        kind: body.kind.into(),
        role: body.role.into(),
        balance: domain::Money::from_minor(body.balance_minor),
        today: state.today().await?,
    };
    let id = db::create_manual_account(&state.pool, &new)
        .await
        .map_err(|error| name_taken(error, Some(&name)))?;
    let account = db::account_balance(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::internal(&"the new account is gone"))?;
    Ok((StatusCode::CREATED, Json(account.into())))
}

/// What's in a manual account today.
#[derive(Deserialize, ToSchema)]
pub struct BalanceUpdate {
    /// In minor units.
    balance_minor: i64,
}

/// `POST /api/v1/accounts/{id}/balance`: records what's in an account with no connection today,
/// as a manual balance snapshot. A second one the same day replaces the first. A balances-only
/// account whose bank link is broken, or whose connection is paused, takes one too.
#[utoipa::path(
    post,
    path = "/api/v1/accounts/{id}/balance",
    operation_id = "updateAccountBalance",
    params(("id" = i64, Path, description = "The account.")),
    request_body = BalanceUpdate,
    responses(
        (status = 204, description = "The balance is recorded."),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such account.", body = ErrorEnvelope),
        (status = 409, description = "`account_connected`: the account has a connection that works, so its balance comes from the bank.", body = ErrorEnvelope),
    ),
)]
pub async fn update_balance(
    State(state): State<AppState>,
    Id(id): Id,
    Body(body): Body<BalanceUpdate>,
) -> Result<StatusCode, ApiError> {
    let today = state.today().await?;
    let balance = domain::Money::from_minor(body.balance_minor);
    let health = crate::sync_health::Snapshot::load(&state).await?;
    if health.takes_manual_balance(id) {
        db::add_snapshot_to_broken_link(&state.pool, id, balance, today).await?;
    } else {
        db::add_manual_snapshot(&state.pool, id, balance, today).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

fn valid_name(typed: &str) -> Result<domain::categories::Name, ApiError> {
    domain::categories::Name::new(typed).map_err(|error| {
        let fields = BTreeMap::from([("name".to_owned(), error.to_string())]);
        ApiError::validation("Check the name.", fields)
    })
}

/// A name taken by another account as the name field's error.
fn name_taken(error: db::AccountError, name: Option<&domain::categories::Name>) -> ApiError {
    match (error, name) {
        (db::AccountError::NameTaken, Some(name)) => {
            let message = format!("There's already an account called {name}.");
            let fields = BTreeMap::from([("name".to_owned(), message)]);
            ApiError::validation("Check the name.", fields)
        }
        (error, _) => error.into(),
    }
}

/// What to change about an account; whatever is left out stays as it is.
#[derive(Deserialize, ToSchema)]
pub struct AccountChange {
    /// Unique among the accounts, ignoring case.
    name: Option<String>,
    /// An empty one clears it *(agent)*.
    institution: Option<String>,
    #[serde(rename = "type")]
    kind: Option<AccountType>,
    role: Option<AccountRole>,
    balances_only: Option<bool>,
    in_net_worth: Option<bool>,
    closed: Option<bool>,
}

/// `PATCH /api/v1/accounts/{id}`: edits an account.
#[utoipa::path(
    patch,
    path = "/api/v1/accounts/{id}",
    operation_id = "changeAccount",
    params(("id" = i64, Path, description = "The account.")),
    request_body = AccountChange,
    responses(
        (status = 204, description = "The account is changed."),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such account.", body = ErrorEnvelope),
        (status = 422, description = "`validation_failed`: the name is empty, too long or taken by another account (any case).", body = ErrorEnvelope),
    ),
)]
pub async fn change(
    State(state): State<AppState>,
    Id(id): Id,
    Body(body): Body<AccountChange>,
) -> Result<StatusCode, ApiError> {
    let name = body.name.as_deref().map(valid_name).transpose()?;
    let change = db::AccountChange {
        name: name.clone(),
        institution: body
            .institution
            .map(|typed| Some(typed.trim().to_owned()).filter(|text| !text.is_empty())),
        kind: body.kind.map(Into::into),
        role: body.role.map(Into::into),
        balances_only: body.balances_only,
        in_net_worth: body.in_net_worth,
        closed: body.closed,
    };
    db::update_account(&state.pool, id, &change)
        .await
        .map_err(|error| name_taken(error, name.as_ref()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema)]
pub struct AccountOrder {
    /// Every account's id exactly once, in order, closed ones included.
    accounts: Vec<i64>,
}

/// `PUT /api/v1/accounts/order`: puts every account in order.
#[utoipa::path(
    put,
    path = "/api/v1/accounts/order",
    operation_id = "orderAccounts",
    request_body = AccountOrder,
    responses(
        (status = 204, description = "The order is saved."),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 409, description = "`accounts_changed`: the order doesn't list every account exactly once.", body = ErrorEnvelope),
    ),
)]
pub async fn order(
    State(state): State<AppState>,
    Body(body): Body<AccountOrder>,
) -> Result<StatusCode, ApiError> {
    db::set_account_order(&state.pool, &body.accounts).await?;
    Ok(StatusCode::NO_CONTENT)
}

impl From<db::AccountError> for ApiError {
    fn from(error: db::AccountError) -> Self {
        match error {
            db::AccountError::NotFound => ApiError::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "There's no such account.",
            ),
            db::AccountError::NameTaken => ApiError::new(
                StatusCode::CONFLICT,
                "name_taken",
                "Another account has that name.",
            ),
            db::AccountError::Order => ApiError::new(
                StatusCode::CONFLICT,
                "accounts_changed",
                "The accounts changed since this screen loaded. Reload and try again.",
            ),
            db::AccountError::Connected => ApiError::new(
                StatusCode::CONFLICT,
                "account_connected",
                "This account has a bank connection, so its balance comes from the bank.",
            ),
            db::AccountError::Db(error) => error.into(),
        }
    }
}
