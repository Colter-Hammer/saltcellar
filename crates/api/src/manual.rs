//! Manual entry (docs/spec/m2-bank-data.md#manual-entry): a transaction typed in by hand, from
//! Transactions' **+**. It lands in a category, or as **Transfer to…** another account; until a
//! bank row adopts it, every field can be edited and it can be deleted. The entry form's payee
//! list comes from here too.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{Body, ErrorEnvelope, Id};
use crate::session::Current;
use crate::transactions::Transaction;
use crate::{ApiError, AppState};

/// A manual transaction, whole: what `POST` writes and `PUT` replaces.
#[derive(Deserialize, ToSchema)]
pub struct ManualEntry {
    /// An account that holds transactions (not balances only).
    account_id: i64,
    /// `YYYY-MM-DD`.
    date: String,
    /// In minor units, not zero: negative is money out, positive money in.
    amount_minor: i64,
    /// The payee's name: an existing payee's exactly, or a new one, added. Blank or null for none.
    #[serde(default)]
    payee: Option<String>,
    /// Its category. Required unless `transfer_account_id` is given; never with it.
    #[serde(default)]
    category_id: Option<i64>,
    /// **Transfer to…** this other account instead of a category: money out goes to it, money in
    /// comes from it. The transfer has only this leg until that account's own row arrives.
    #[serde(default)]
    transfer_account_id: Option<i64>,
    #[serde(default)]
    memo: Option<String>,
    /// Tag names, with or without `#`: letters, digits, `-` and `_`. New ones are added.
    #[serde(default)]
    tags: Vec<String>,
}

impl ManualEntry {
    /// The entry checked, or every field's problem at once.
    fn check(&self) -> Result<domain::manual::Entry, ApiError> {
        domain::manual::check(&domain::manual::Typed {
            account_id: self.account_id,
            date: &self.date,
            amount_minor: self.amount_minor,
            payee: self.payee.as_deref(),
            category_id: self.category_id,
            transfer_account_id: self.transfer_account_id,
            memo: self.memo.as_deref(),
            tags: &self.tags,
        })
        .map_err(|errors| {
            invalid(
                errors
                    .into_iter()
                    .map(|(field, message)| (field.to_owned(), message))
                    .collect(),
            )
        })
    }
}

fn invalid(fields: BTreeMap<String, String>) -> ApiError {
    ApiError::validation("Check the transaction.", fields)
}

fn field(name: &str, message: &str) -> ApiError {
    invalid(BTreeMap::from([(name.to_owned(), message.to_owned())]))
}

fn not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "not_found",
        "There's no such transaction.",
    )
}

fn refused(error: db::ManualError) -> ApiError {
    match error {
        db::ManualError::NoSuchAccount => field("account_id", "There's no such account."),
        db::ManualError::BalancesOnly => field(
            "account_id",
            "This account holds balances only, so it takes no transactions.",
        ),
        db::ManualError::NoSuchCategory => field("category_id", "There's no such category."),
        db::ManualError::NoSuchTransferAccount => {
            field("transfer_account_id", "There's no such account.")
        }
        db::ManualError::NotFound => not_found(),
        db::ManualError::NotManual => ApiError::new(
            StatusCode::CONFLICT,
            "not_manual",
            "Only a manual transaction the bank hasn't sent yet can be changed or deleted.",
        ),
        db::ManualError::Db(error) => error.into(),
    }
}

async fn detail(state: &AppState, id: i64) -> Result<Transaction, ApiError> {
    let detail = db::transaction(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::internal(&"a manual transaction vanished as it was written"))?;
    Ok(detail.into())
}

/// `POST /api/v1/transactions`: enters a manual transaction. It's posted *(agent)*, counts in
/// budgets at once, records `categorized_by = 'user'` with the reason "Set by <display name>",
/// and shows the manual badge until a bank row adopts it.
#[utoipa::path(
    post,
    path = "/api/v1/transactions",
    operation_id = "addTransaction",
    request_body = ManualEntry,
    responses(
        (status = 201, description = "The transaction is entered.", body = Transaction),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 422, description = "`validation_failed`: a field can't be written as given, named in `fields`: no category and no transfer, both, a transfer to the same account, an account, category or transfer account that doesn't exist, a balances-only account, a zero amount, an unreadable date, a bad tag, or a payee or memo too long.", body = ErrorEnvelope),
    ),
)]
pub async fn create(
    State(state): State<AppState>,
    Extension(current): Extension<Current>,
    Body(body): Body<ManualEntry>,
) -> Result<(StatusCode, Json<Transaction>), ApiError> {
    let entry = body.check()?;
    let author = db::Author {
        user_id: current.login.user_id,
        display_name: &current.login.display_name,
        at: state.now(),
    };
    let id = db::add_manual_transaction(&state.pool, &entry, &author)
        .await
        .map_err(refused)?;
    Ok((StatusCode::CREATED, Json(detail(&state, id).await?)))
}

/// `PUT /api/v1/transactions/{id}`: replaces every field of a manual transaction no bank row has
/// adopted, its tags and its transfer included.
#[utoipa::path(
    put,
    path = "/api/v1/transactions/{id}",
    operation_id = "changeTransaction",
    params(("id" = i64, Path, description = "The transaction.")),
    request_body = ManualEntry,
    responses(
        (status = 200, description = "The transaction, changed.", body = Transaction),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such transaction.", body = ErrorEnvelope),
        (status = 409, description = "`not_manual`: it came from the bank or a file, or a bank row has adopted it.", body = ErrorEnvelope),
        (status = 422, description = "`validation_failed`: as for `POST /api/v1/transactions`.", body = ErrorEnvelope),
    ),
)]
pub async fn change(
    State(state): State<AppState>,
    Extension(current): Extension<Current>,
    Id(id): Id,
    Body(body): Body<ManualEntry>,
) -> Result<Json<Transaction>, ApiError> {
    let entry = body.check()?;
    let author = db::Author {
        user_id: current.login.user_id,
        display_name: &current.login.display_name,
        at: state.now(),
    };
    db::update_manual_transaction(&state.pool, id, &entry, &author)
        .await
        .map_err(refused)?;
    Ok(Json(detail(&state, id).await?))
}

/// `DELETE /api/v1/transactions/{id}`: deletes a manual transaction no bank row has adopted,
/// with its tags and its transfer (the screen's **Undo** enters it again *(agent)*), or a row from
/// the bank (docs/spec/m2-bank-data.md#deleting-a-synced-row). A bank row's id is kept, so the
/// next sync doesn't bring it back; the screen warns first and suggests excluding it from budgets
/// instead. A row from a file without a bank id is undone with its import instead.
#[utoipa::path(
    delete,
    path = "/api/v1/transactions/{id}",
    operation_id = "deleteTransaction",
    params(("id" = i64, Path, description = "The transaction.")),
    responses(
        (status = 204, description = "The transaction is deleted."),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
        (status = 404, description = "`not_found`: there's no such transaction.", body = ErrorEnvelope),
        (status = 409, description = "`not_manual`: it came from a file and has no bank id.", body = ErrorEnvelope),
    ),
)]
pub async fn delete(
    State(state): State<AppState>,
    Extension(current): Extension<Current>,
    Id(id): Id,
) -> Result<StatusCode, ApiError> {
    match db::delete_manual_transaction(&state.pool, id).await {
        Err(db::ManualError::NotManual) => {}
        other => {
            other.map_err(refused)?;
            return Ok(StatusCode::NO_CONTENT);
        }
    }
    db::delete_bank_row(&state.pool, id, current.login.user_id, state.now())
        .await
        .map_err(|error| match error {
            db::DeleteBankRowError::NotFound => refused(db::ManualError::NotFound),
            db::DeleteBankRowError::NotFromBank => ApiError::new(
                StatusCode::CONFLICT,
                "not_manual",
                "A transaction from a file is removed by undoing its import.",
            ),
            db::DeleteBankRowError::Db(error) => error.into(),
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// A payee, as the entry form offers it.
#[derive(Serialize, ToSchema)]
pub struct Payee {
    id: i64,
    name: String,
}

/// Every payee the household has.
#[derive(Serialize, ToSchema)]
pub struct PayeeList {
    /// By name, ignoring case.
    payees: Vec<Payee>,
}

/// `GET /api/v1/payees`: every payee, for the entry form's payee field.
#[utoipa::path(
    get,
    path = "/api/v1/payees",
    operation_id = "getPayees",
    responses(
        (status = 200, description = "Every payee, by name.", body = PayeeList),
        (status = 401, description = "`login_required`: no live session.", body = ErrorEnvelope),
    ),
)]
pub async fn payees(State(state): State<AppState>) -> Result<Json<PayeeList>, ApiError> {
    let payees = db::payees(&state.pool).await?;
    Ok(Json(PayeeList {
        payees: payees
            .into_iter()
            .map(|payee| Payee {
                id: payee.id,
                name: payee.name,
            })
            .collect(),
    }))
}
