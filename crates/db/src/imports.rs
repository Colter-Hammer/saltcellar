//! Import batches: the Actual import's commit (docs/spec/m1-ledger-import.md#step-4-done).

use std::collections::HashMap;

use domain::accounts::{Opening, app_balance};
use domain::import::{Disposition, LinkedTransfer, Plan, PlannedAccount, Row, Target, dedupe_key};
use domain::{Date, Money};
use sqlx::SqliteConnection;

use crate::{Error, SqlitePool};

/// Whether an import from Actual is in place and not undone. Until milestone 2 makes re-importing
/// overlap-aware, a second one is refused (docs/spec/m1-ledger-import.md#step-1-file).
///
/// # Errors
/// If the database fails.
pub async fn actual_import_in_place(pool: &SqlitePool) -> Result<bool, Error> {
    let mut conn = pool.acquire().await?;
    Ok(in_place(&mut conn).await?)
}

async fn in_place(conn: &mut SqliteConnection) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (
             SELECT 1 FROM import_batches WHERE format = 'actual' AND undone_at IS NULL
           ) AS "in_place!: bool""#
    )
    .fetch_one(conn)
    .await
}

/// An Actual import to commit: the plan from the importer's choices, and who's importing what.
#[derive(Debug, Clone, Copy)]
pub struct ActualCommit<'a> {
    pub plan: &'a Plan,
    /// The uploaded file's name, for Settings › Imports.
    pub file_name: &'a str,
    /// The login importing.
    pub imported_by: i64,
    /// The import day in the household timezone: the opening balance's date for an account with
    /// no rows, and its snapshot's.
    pub today: Date,
}

/// What a committed import wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Committed {
    pub batch_id: i64,
    /// One per account the plan has, in its order.
    pub accounts: Vec<CommittedAccount>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedAccount {
    pub id: i64,
    pub name: String,
    /// Whether the import created it (or it was the same as one there already).
    pub created: bool,
    /// The rows imported into it.
    pub rows: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("an earlier import from Actual hasn't been undone")]
    EarlierImport,
    /// An account whose balance less its rows doesn't fit, or whose first row has no day before.
    #[error("the balance of {0} can't be worked out")]
    Balance(String),
    #[error(transparent)]
    Db(#[from] Error),
}

impl From<sqlx::Error> for ImportError {
    fn from(error: sqlx::Error) -> Self {
        ImportError::Db(Error::Sqlx(error))
    }
}

/// Commits an Actual import in one database transaction: the accounts it creates, its rows with
/// their payees, aliases, tags and split lines, the transfers linking their legs, each account's opening balance and balance snapshot, and the
/// import batch recording it all, so it can be undone. On any failure nothing is written.
///
/// # Errors
/// [`ImportError::EarlierImport`] while an earlier Actual import is in place;
/// [`ImportError::Balance`] for an account whose balance can't be worked out; otherwise only if
/// the database fails.
pub async fn commit_actual_import(
    pool: &SqlitePool,
    commit: ActualCommit<'_>,
) -> Result<Committed, ImportError> {
    // Immediate, so two commits at once can't both find no earlier import.
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    if in_place(&mut tx).await? {
        return Err(ImportError::EarlierImport);
    }
    let skipped = i64::try_from(commit.plan.skipped).unwrap_or(i64::MAX);
    let batch_id = sqlx::query_scalar!(
        r#"INSERT INTO import_batches (format, file_name, added, matched, skipped, imported_by)
           VALUES ('actual', ?, 0, 0, ?, ?) RETURNING id AS "id!: i64""#,
        commit.file_name,
        skipped,
        commit.imported_by
    )
    .fetch_one(&mut *tx)
    .await?;

    let mut writer = Writer {
        tx: &mut tx,
        batch_id,
        payees: HashMap::new(),
        tags: HashMap::new(),
        inserted: HashMap::new(),
    };
    let mut accounts = Vec::with_capacity(commit.plan.accounts.len());
    let mut added: i64 = 0;
    for planned in &commit.plan.accounts {
        let account = writer.account(planned, commit.today).await?;
        added += i64::try_from(account.rows).unwrap_or(i64::MAX);
        accounts.push(account);
    }
    for transfer in &commit.plan.transfers {
        writer.transfer(transfer).await?;
    }
    sqlx::query!(
        "UPDATE import_batches SET added = ? WHERE id = ?",
        added,
        batch_id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Committed { batch_id, accounts })
}

/// Writes inside the commit's transaction, remembering the payees and tags it has found or made.
struct Writer<'t> {
    tx: &'t mut SqliteConnection,
    batch_id: i64,
    payees: HashMap<String, i64>,
    tags: HashMap<String, i64>,
    /// Each row inserted so far by its Actual id: its transaction's id and its account's.
    inserted: HashMap<String, (i64, i64)>,
}

impl Writer<'_> {
    /// One account: created or found, its rows, opening balance and snapshot.
    async fn account(
        &mut self,
        planned: &PlannedAccount,
        today: Date,
    ) -> Result<CommittedAccount, ImportError> {
        let (id, name, created, balances_only) = match &planned.target {
            Target::New(new) => {
                let id = sqlx::query_scalar!(
                    r#"INSERT INTO accounts
                         (name, type, role, has_transactions, closed, sort_order, external_account_id)
                       VALUES (?, ?, ?, ?, ?, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM accounts), ?)
                       RETURNING id AS "id!: i64""#,
                    new.name,
                    new.kind.as_str(),
                    new.role.as_str(),
                    !new.balances_only,
                    new.closed,
                    new.bank_id
                )
                .fetch_one(&mut *self.tx)
                .await?;
                (id, new.name.clone(), true, new.balances_only)
            }
            Target::Existing(id) => {
                let found = sqlx::query!(
                    r#"SELECT name, has_transactions AS "has_transactions: bool"
                       FROM accounts WHERE id = ?"#,
                    id
                )
                .fetch_one(&mut *self.tx)
                .await?;
                (*id, found.name, false, !found.has_transactions)
            }
        };
        for row in &planned.rows {
            self.row(id, row).await?;
        }

        let as_of = planned.as_of.unwrap_or(today).to_string();
        let balance = planned.balance.minor();
        let app = if balances_only {
            None
        } else {
            let opening = self.opening(id, planned.balance, today).await?;
            let unbalanced = || ImportError::Balance(name.clone());
            let opening = opening.ok_or_else(unbalanced)?;
            let rows = self.total(id).await?;
            let app = app_balance(opening.balance, [rows]).ok_or_else(unbalanced)?;
            let (opening_balance, opening_date) =
                (opening.balance.minor(), opening.date.to_string());
            sqlx::query!(
                "UPDATE accounts SET opening_balance_minor = ?, opening_date = ? WHERE id = ?",
                opening_balance,
                opening_date,
                id
            )
            .execute(&mut *self.tx)
            .await?;
            Some(app.minor())
        };
        sqlx::query!(
            "INSERT INTO balance_snapshots
               (account_id, as_of, balance_minor, source, app_balance_posted_minor, app_balance_pending_minor)
             VALUES (?, ?, ?, 'import', ?, ?)
             ON CONFLICT (account_id, as_of) DO UPDATE SET
               balance_minor = excluded.balance_minor, source = excluded.source,
               app_balance_posted_minor = excluded.app_balance_posted_minor,
               app_balance_pending_minor = excluded.app_balance_pending_minor",
            id,
            as_of,
            balance,
            app,
            app
        )
        .execute(&mut *self.tx)
        .await?;
        Ok(CommittedAccount {
            id,
            name,
            created,
            rows: planned.rows.len() as u64,
        })
    }

    /// The opening balance from the account's Actual balance and every row it has now.
    async fn opening(
        &mut self,
        account: i64,
        balance: Money,
        today: Date,
    ) -> Result<Option<Opening>, ImportError> {
        let earliest = sqlx::query_scalar!(
            r#"SELECT MIN(date) AS "earliest: String" FROM transactions WHERE account_id = ?"#,
            account
        )
        .fetch_one(&mut *self.tx)
        .await?;
        let earliest = earliest
            .map(|text| {
                text.parse::<Date>().map_err(|_| Error::Unreadable {
                    what: "a transaction date",
                    value: text,
                })
            })
            .transpose()?;
        let rows = self.total(account).await?;
        Ok(Opening::from_import(balance, rows, earliest, today))
    }

    /// The sum of the account's transactions.
    async fn total(&mut self, account: i64) -> Result<Money, ImportError> {
        let total = sqlx::query_scalar!(
            r#"SELECT COALESCE(SUM(amount_minor), 0) AS "total!: i64"
               FROM transactions WHERE account_id = ?"#,
            account
        )
        .fetch_one(&mut *self.tx)
        .await?;
        Ok(Money::from_minor(total))
    }

    async fn row(&mut self, account: i64, row: &Row) -> Result<(), ImportError> {
        let payee = match &row.payee {
            Some(name) => Some(self.payee(name, row.alias.as_deref()).await?),
            None => None,
        };
        let (category, categorized_by, reason) = match &row.disposition {
            Disposition::Mapped {
                category_id,
                reason,
            } => (Some(*category_id), Some("import"), Some(reason.as_str())),
            Disposition::Split { reason, .. } => (None, Some("import"), Some(reason.as_str())),
            _ => (None, None, None),
        };
        let date = row.date.to_string();
        let amount = row.amount.minor();
        let dedupe = dedupe_key(account, row.date, row.amount, &row.raw_description);
        let id = sqlx::query_scalar!(
            r#"INSERT INTO transactions
                 (account_id, date, amount_minor, status, payee_id, raw_description, memo, category_id,
                  categorized_by, categorization_reason, source, external_id, dedupe_key)
               VALUES (?, ?, ?, 'posted', ?, ?, ?, ?, ?, ?, 'import', ?, ?)
               RETURNING id AS "id!: i64""#,
            account,
            date,
            amount,
            payee,
            row.raw_description,
            row.memo,
            category,
            categorized_by,
            reason,
            row.external_id,
            dedupe
        )
        .fetch_one(&mut *self.tx)
        .await?;
        if let Disposition::Split { lines, .. } = &row.disposition {
            for line in lines {
                let amount = line.amount.minor();
                sqlx::query!(
                    "INSERT INTO transaction_splits (transaction_id, category_id, amount_minor, memo)
                     VALUES (?, ?, ?, ?)",
                    id,
                    line.category_id,
                    amount,
                    line.memo
                )
                .execute(&mut *self.tx)
                .await?;
            }
        }
        self.inserted.insert(row.import_ref.clone(), (id, account));
        for tag in &row.tags {
            let tag = self.tag(tag).await?;
            sqlx::query!(
                "INSERT INTO transaction_tags (transaction_id, tag_id) VALUES (?, ?)",
                id,
                tag
            )
            .execute(&mut *self.tx)
            .await?;
        }
        let disposition = row.disposition.as_str();
        sqlx::query!(
            "INSERT INTO import_batch_rows
               (batch_id, transaction_id, outcome, import_ref, source_category, source_disposition)
             VALUES (?, ?, 'inserted', ?, ?, ?)",
            self.batch_id,
            id,
            row.import_ref,
            row.source_category,
            disposition
        )
        .execute(&mut *self.tx)
        .await?;
        Ok(())
    }

    /// The `transfers` row for a pair of inserted legs, which are now transfer legs.
    async fn transfer(&mut self, transfer: &LinkedTransfer) -> Result<(), ImportError> {
        let leg = |import_ref: &str| {
            self.inserted
                .get(import_ref)
                .copied()
                .ok_or_else(|| Error::Unreadable {
                    what: "a linked transfer's leg",
                    value: import_ref.to_owned(),
                })
        };
        let (from_txn, from_account) = leg(&transfer.from)?;
        let (to_txn, to_account) = leg(&transfer.to)?;
        sqlx::query!(
            "INSERT INTO transfers
               (from_account_id, to_account_id, from_txn_id, to_txn_id, matched_by, state)
             VALUES (?, ?, ?, ?, 'import', 'confirmed')",
            from_account,
            to_account,
            from_txn,
            to_txn
        )
        .execute(&mut *self.tx)
        .await?;
        sqlx::query!(
            "UPDATE transactions SET categorized_by = 'transfer' WHERE id IN (?, ?)",
            from_txn,
            to_txn
        )
        .execute(&mut *self.tx)
        .await?;
        Ok(())
    }

    /// The payee with exactly this name, made if new; `alias` becomes one of its aliases if no
    /// payee has it yet.
    async fn payee(&mut self, name: &str, alias: Option<&str>) -> Result<i64, ImportError> {
        let id = if let Some(id) = self.payees.get(name) {
            *id
        } else {
            sqlx::query!(
                "INSERT INTO payees (name) VALUES (?) ON CONFLICT (name) DO NOTHING",
                name
            )
            .execute(&mut *self.tx)
            .await?;
            let id = sqlx::query_scalar!(
                r#"SELECT id AS "id!: i64" FROM payees WHERE name = ?"#,
                name
            )
            .fetch_one(&mut *self.tx)
            .await?;
            self.payees.insert(name.to_owned(), id);
            id
        };
        if let Some(alias) = alias {
            sqlx::query!(
                "INSERT INTO payee_aliases (normalized_text, payee_id) VALUES (?, ?)
                 ON CONFLICT (normalized_text) DO NOTHING",
                alias,
                id
            )
            .execute(&mut *self.tx)
            .await?;
        }
        Ok(id)
    }

    /// The tag with exactly this name, made if new.
    async fn tag(&mut self, name: &str) -> Result<i64, ImportError> {
        if let Some(id) = self.tags.get(name) {
            return Ok(*id);
        }
        sqlx::query!(
            "INSERT INTO tags (name) VALUES (?) ON CONFLICT (name) DO NOTHING",
            name
        )
        .execute(&mut *self.tx)
        .await?;
        let id = sqlx::query_scalar!(r#"SELECT id AS "id!: i64" FROM tags WHERE name = ?"#, name)
            .fetch_one(&mut *self.tx)
            .await?;
        self.tags.insert(name.to_owned(), id);
        Ok(id)
    }
}

#[cfg(test)]
mod tests;
