//! Accounts (docs/spec/m1-ledger-import.md#accounts): read-only in milestone 1, where the Actual
//! importer creates them.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{ErrorEnvelope, Id};
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
    /// Balances only: it holds balance snapshots and no transactions.
    balances_only: bool,
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
            balances_only: !account.has_transactions,
            balance_minor: account.snapshot.map(domain::Money::minor),
            app_balance_minor: account.app_balance.map(domain::Money::minor),
            opening_balance_minor: account.opening.map(|opening| opening.balance.minor()),
            opening_date: account.opening.map(|opening| opening.date.to_string()),
            rows: account.rows,
            last_transaction_date: account.last_transaction.map(|date| date.to_string()),
        }
    }
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
    Ok(Json(AccountList {
        accounts: accounts.into_iter().map(Account::from).collect(),
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
    Ok(Json(account.into()))
}
