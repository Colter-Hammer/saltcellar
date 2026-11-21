//! Import batches: the Actual import's commit (docs/spec/m1-ledger-import.md#step-4-done).

use std::collections::HashMap;

use std::collections::HashSet;

use domain::accounts::{Opening, app_balance};
use domain::actual_overlap::{Overlap, StoredRow, overlap};
use domain::import::{
    Disposition, LinkedTransfer, Plan, PlannedAccount, Row, SplitLine, Target, TrackedAccount,
    dedupe_key,
};
use domain::{Date, Money};
use sqlx::SqliteConnection;

use crate::{Error, SqlitePool};

/// What becomes of each row of each planned account, as [`Plan::accounts`] lists them: matched
/// against the rows an account the import adds to has already
/// (docs/spec/m2-bank-data.md#import-from-actual-becomes-overlap-aware). An account the import
/// creates or tracks has none, so every row is new.
pub type Overlaps = Vec<Vec<Overlap>>;

/// What committing `plan` would do with each row, as the preview shows it.
///
/// # Errors
/// If the database fails or holds a date it can't read.
pub async fn actual_overlaps(pool: &SqlitePool, plan: &Plan) -> Result<Overlaps, Error> {
    let mut conn = pool.acquire().await?;
    overlaps(&mut conn, plan).await
}

async fn overlaps(conn: &mut SqliteConnection, plan: &Plan) -> Result<Overlaps, Error> {
    let mut all = Vec::with_capacity(plan.accounts.len());
    for planned in &plan.accounts {
        all.push(match planned.target {
            Target::Existing(id) => {
                let (stored, synced_from) = stored_rows(conn, id).await?;
                overlap(&planned.rows, &stored, synced_from)
            }
            Target::New(_) | Target::Track(_) => vec![Overlap::New; planned.rows.len()],
        });
    }
    Ok(all)
}

/// Account `id`'s rows, each with the Actual id an earlier import not undone brought it in under,
/// and the date of its oldest synced row.
async fn stored_rows(
    conn: &mut SqliteConnection,
    id: i64,
) -> Result<(Vec<StoredRow>, Option<Date>), Error> {
    let date = |text: String| {
        text.parse::<Date>().map_err(|_| Error::Unreadable {
            what: "a transaction date",
            value: text,
        })
    };
    let rows = sqlx::query!(
        r#"SELECT t.id AS "id!: i64", t.external_id, t.date, t.amount_minor, t.raw_description,
             (SELECT r.import_ref FROM import_batch_rows r
                JOIN import_batches b ON b.id = r.batch_id
               WHERE r.transaction_id = t.id AND b.format = 'actual' AND b.undone_at IS NULL
               ORDER BY r.batch_id LIMIT 1) AS "actual_id?: String"
           FROM transactions t WHERE t.account_id = ? ORDER BY t.date, t.id"#,
        id
    )
    .fetch_all(&mut *conn)
    .await?;
    let stored = rows
        .into_iter()
        .map(|row| {
            Ok(StoredRow {
                id: row.id,
                external_id: row.external_id,
                actual_id: row.actual_id,
                date: date(row.date)?,
                amount: Money::from_minor(row.amount_minor),
                raw_description: row.raw_description,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let synced_from = sqlx::query_scalar!(
        r#"SELECT MIN(date) AS "date: String" FROM transactions
           WHERE account_id = ? AND source = 'provider'"#,
        id
    )
    .fetch_one(&mut *conn)
    .await?
    .map(date)
    .transpose()?;
    Ok((stored, synced_from))
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
    /// The Actual ids of the possible duplicates the household said are new: inserted. Any other
    /// is taken as the stored row it may be.
    pub new_rows: &'a HashSet<String>,
}

/// What a committed import wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Committed {
    pub batch_id: i64,
    /// One per account the plan has, in its order.
    pub accounts: Vec<CommittedAccount>,
    /// The transfers linked: those of the plan whose two legs were both inserted.
    pub linked_transfers: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedAccount {
    pub id: i64,
    pub name: String,
    /// Whether the import created it (or it was the same as one there already).
    pub created: bool,
    /// The rows imported into it.
    pub rows: u64,
    /// Its rows that were there already: synced, or brought by an earlier import.
    pub matched: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// A bank account the import was to track isn't waiting in the prompt any more.
    #[error("the bank account {0} isn't waiting any more")]
    NotWaiting(String),
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

/// Commits an Actual import in one database transaction: the accounts it creates or tracks, its
/// new rows with their payees, aliases, tags and split lines, what it fills on the rows that were
/// there already (recorded in `prior_fields` for Undo), the transfers linking new legs, each
/// account's opening balance and balance snapshot, and the import batch recording it all, so it
/// can be undone. The rows are matched again inside the transaction. On any failure nothing is
/// written.
///
/// # Errors
/// [`ImportError::NotWaiting`] for a bank account to track that isn't waiting any more;
/// [`ImportError::Balance`] for an account whose balance can't be worked out; otherwise only if
/// the database fails.
pub async fn commit_actual_import(
    pool: &SqlitePool,
    commit: ActualCommit<'_>,
) -> Result<Committed, ImportError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let overlaps = overlaps(&mut tx, commit.plan).await?;
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
        new_rows: commit.new_rows,
        reserved: commit
            .plan
            .accounts
            .iter()
            .filter_map(|planned| match &planned.target {
                Target::New(new) => Some(new.name.clone()),
                _ => None,
            })
            .collect(),
    };
    let mut accounts = Vec::with_capacity(commit.plan.accounts.len());
    let (mut added, mut matched) = (0_u64, 0_u64);
    for (planned, overlaps) in commit.plan.accounts.iter().zip(&overlaps) {
        let account = writer.account(planned, overlaps, commit.today).await?;
        added += account.rows;
        matched += account.matched;
        accounts.push(account);
    }
    let mut linked = 0;
    for transfer in &commit.plan.transfers {
        linked += u64::from(writer.transfer(transfer).await?);
    }
    let (added, matched) = (
        i64::try_from(added).unwrap_or(i64::MAX),
        i64::try_from(matched).unwrap_or(i64::MAX),
    );
    sqlx::query!(
        "UPDATE import_batches SET added = ?, matched = ? WHERE id = ?",
        added,
        matched,
        batch_id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Committed {
        batch_id,
        accounts,
        linked_transfers: linked,
    })
}

/// Writes inside the commit's transaction, remembering the payees and tags it has found or made.
struct Writer<'t> {
    tx: &'t mut SqliteConnection,
    batch_id: i64,
    payees: HashMap<String, i64>,
    tags: HashMap<String, i64>,
    /// Each row inserted so far by its Actual id: its transaction's id and its account's.
    inserted: HashMap<String, (i64, i64)>,
    new_rows: &'t HashSet<String>,
    /// The names of the accounts the import creates, which an account it tracks doesn't take.
    reserved: Vec<String>,
}

impl Writer<'_> {
    /// One account: created or found, its rows, opening balance and snapshot.
    async fn account(
        &mut self,
        planned: &PlannedAccount,
        overlaps: &[Overlap],
        today: Date,
    ) -> Result<CommittedAccount, ImportError> {
        let (id, name, created, balances_only) = match &planned.target {
            Target::New(new) => {
                let id = sqlx::query_scalar!(
                    r#"INSERT INTO accounts
                         (name, type, role, has_transactions, closed, sort_order, external_account_id,
                          created_by_batch_id)
                       VALUES (?, ?, ?, ?, ?, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM accounts), ?, ?)
                       RETURNING id AS "id!: i64""#,
                    new.name,
                    new.kind.as_str(),
                    new.role.as_str(),
                    !new.balances_only,
                    new.closed,
                    new.bank_id,
                    self.batch_id
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
            Target::Track(tracked) => {
                let (id, name) = self.track(tracked).await?;
                (id, name, true, tracked.balances_only)
            }
        };
        let (inserted, matched) = self.rows(id, &planned.rows, overlaps).await?;
        if !created && self.synced(id).await? {
            // A synced account's balance is the bank's: its rows from Actual are older history,
            // which moves the opening balance back so drift stays as it was, and Actual's
            // balance isn't written over the bank's (docs/spec/m2-bank-data.md#balances).
            self.move_opening_back(id).await?;
            return Ok(CommittedAccount {
                id,
                name,
                created,
                rows: inserted,
                matched,
            });
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
            rows: inserted,
            matched,
        })
    }

    /// An account's rows: those that are new inserted, those there already filled. Resolves to
    /// how many of each.
    async fn rows(
        &mut self,
        account: i64,
        rows: &[Row],
        overlaps: &[Overlap],
    ) -> Result<(u64, u64), ImportError> {
        let (mut inserted, mut matched) = (0, 0);
        for (row, overlap) in rows.iter().zip(overlaps) {
            match *overlap {
                Overlap::Seen(_) => matched += 1,
                Overlap::Check(_) if self.new_rows.contains(&row.import_ref) => {
                    self.row(account, row).await?;
                    inserted += 1;
                }
                Overlap::SameId(stored) | Overlap::Matched(stored) | Overlap::Check(stored) => {
                    self.fill(stored, row).await?;
                    matched += 1;
                }
                Overlap::New => {
                    self.row(account, row).await?;
                    inserted += 1;
                }
            }
        }
        Ok((inserted, matched))
    }

    /// Tracks the waiting bank account as a new account, as the new accounts prompt would: named
    /// as the bank names it (numbered if taken), of the type its name suggests, with the role and
    /// balances-only chosen. Resolves to its id and name.
    async fn track(&mut self, tracked: &TrackedAccount) -> Result<(i64, String), ImportError> {
        let waiting = sqlx::query!(
            r#"SELECT connection_id AS "connection_id!: i64", provider_name, institution
               FROM connection_accounts
               WHERE connection_id = (SELECT id FROM connections ORDER BY id LIMIT 1)
                 AND external_account_id = ? AND decision = 'pending'"#,
            tracked.external_id
        )
        .fetch_optional(&mut *self.tx)
        .await?
        .ok_or_else(|| ImportError::NotWaiting(tracked.external_id.clone()))?;
        let name = crate::connection_accounts::free_name(
            &mut *self.tx,
            &waiting.provider_name,
            &self.reserved,
        )
        .await?;
        let kind = domain::accounts::AccountType::guess(&name, tracked.role).as_str();
        let role = tracked.role.as_str();
        let has_transactions = !tracked.balances_only;
        let id = sqlx::query_scalar!(
            r#"INSERT INTO accounts
                 (name, institution, type, role, has_transactions, sort_order, created_by_batch_id)
               VALUES (?, ?, ?, ?, ?, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM accounts), ?)
               RETURNING id AS "id!: i64""#,
            name,
            waiting.institution,
            kind,
            role,
            has_transactions,
            self.batch_id
        )
        .fetch_one(&mut *self.tx)
        .await?;
        crate::connection_accounts::link(
            &mut *self.tx,
            waiting.connection_id,
            &tracked.external_id,
            id,
        )
        .await?;
        Ok((id, name))
    }

    /// A row that was there already takes what Actual adds: its mapped category (or split lines),
    /// unless the household set the category, it's a transfer leg or it's split already; its
    /// notes, if it has none; its payee, unless the household entered the row or edited it; and
    /// the notes' tags. It keeps its amount, date, status
    /// and text. What it had is recorded in `prior_fields` for Undo.
    async fn fill(&mut self, id: i64, row: &Row) -> Result<(), ImportError> {
        let had = sqlx::query!(
            r#"SELECT category_id, categorized_by, categorization_reason, memo, payee_id,
                 EXISTS (SELECT 1 FROM transaction_splits s WHERE s.transaction_id = t.id)
                   AS "split!: bool",
                 (source = 'manual' OR updated_by IS NOT NULL) AS "households!: bool"
               FROM transactions t WHERE t.id = ?"#,
            id
        )
        .fetch_one(&mut *self.tx)
        .await?;
        let (category, categorized_by, reason, lines) = filled_category(
            &row.disposition,
            Category {
                id: had.category_id,
                by: had.categorized_by.clone(),
                reason: had.categorization_reason.clone(),
            },
            had.split,
        );
        let memo = match had.memo.as_deref().map(str::trim) {
            Some(memo) if !memo.is_empty() => had.memo.clone(),
            _ => row.memo.clone().or_else(|| had.memo.clone()),
        };
        // The feed's payee gives way to Actual's; one the household typed or chose doesn't.
        let payee = match &row.payee {
            Some(name) if !had.households => Some(self.payee(name, row.alias.as_deref()).await?),
            _ => had.payee_id,
        };
        sqlx::query!(
            "UPDATE transactions SET category_id = ?, categorized_by = ?,
               categorization_reason = ?, memo = ?, payee_id = ?
             WHERE id = ?",
            category,
            categorized_by,
            reason,
            memo,
            payee,
            id
        )
        .execute(&mut *self.tx)
        .await?;
        self.split_lines(id, lines).await?;
        let tags_added = self.add_tags(id, &row.tags).await?;
        let prior = serde_json::json!({
            "category_id": had.category_id,
            "categorized_by": had.categorized_by,
            "categorization_reason": had.categorization_reason,
            "memo": had.memo,
            "payee_id": had.payee_id,
            "split_added": !lines.is_empty(),
            "tags_added": tags_added,
        })
        .to_string();
        let disposition = row.disposition.as_str();
        sqlx::query!(
            "INSERT INTO import_batch_rows
               (batch_id, transaction_id, outcome, import_ref, source_category, source_disposition,
                prior_fields)
             VALUES (?, ?, 'matched', ?, ?, ?, ?)",
            self.batch_id,
            id,
            row.import_ref,
            row.source_category,
            disposition,
            prior
        )
        .execute(&mut *self.tx)
        .await?;
        Ok(())
    }

    /// Tags row `id` with each of `tags` it doesn't have yet; resolves to the ones added.
    async fn add_tags(&mut self, id: i64, tags: &[String]) -> Result<Vec<i64>, ImportError> {
        let mut added = Vec::new();
        for tag in tags {
            let tag = self.tag(tag).await?;
            let rows = sqlx::query!(
                "INSERT INTO transaction_tags (transaction_id, tag_id) VALUES (?, ?)
                 ON CONFLICT DO NOTHING",
                id,
                tag
            )
            .execute(&mut *self.tx)
            .await?
            .rows_affected();
            if rows > 0 {
                added.push(tag);
            }
        }
        Ok(added)
    }

    async fn split_lines(&mut self, id: i64, lines: &[SplitLine]) -> Result<(), ImportError> {
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
        Ok(())
    }

    /// Moves account `id`'s opening balance back over the older rows just imported, keeping the
    /// one it had for Undo.
    async fn move_opening_back(&mut self, id: i64) -> Result<(), ImportError> {
        if let Some(before) = crate::balances::move_opening_back(self.tx, id).await? {
            let (minor, date) = (before.balance.minor(), before.date.to_string());
            sqlx::query!(
                "INSERT INTO import_batch_openings
                   (batch_id, account_id, opening_balance_minor, opening_date)
                 VALUES (?, ?, ?, ?)",
                self.batch_id,
                id,
                minor,
                date
            )
            .execute(&mut *self.tx)
            .await?;
        }
        Ok(())
    }

    /// Whether account `id` is synced: it has an opening balance and a bank's snapshot.
    async fn synced(&mut self, id: i64) -> Result<bool, ImportError> {
        Ok(sqlx::query_scalar!(
            r#"SELECT EXISTS (
                 SELECT 1 FROM accounts a
                 WHERE a.id = ?1 AND a.opening_balance_minor IS NOT NULL
                   AND EXISTS (SELECT 1 FROM balance_snapshots s
                               WHERE s.account_id = a.id AND s.source = 'provider')
               ) AS "synced!: bool""#,
            id
        )
        .fetch_one(&mut *self.tx)
        .await?)
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
            self.split_lines(id, lines).await?;
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

    /// The `transfers` row for a pair of inserted legs, which are now transfer legs. A pair with a
    /// leg that was there already isn't linked *(agent)*: both stay transfer-like. Resolves to
    /// whether it linked them.
    async fn transfer(&mut self, transfer: &LinkedTransfer) -> Result<bool, ImportError> {
        let (Some(&(from_txn, from_account)), Some(&(to_txn, to_account))) = (
            self.inserted.get(&transfer.from),
            self.inserted.get(&transfer.to),
        ) else {
            return Ok(false);
        };
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
        Ok(true)
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

/// A row's category, how it was set and why.
struct Category {
    id: Option<i64>,
    by: Option<String>,
    reason: Option<String>,
}

/// The category a row there already ends up with, and the split lines it gains: Actual's mapped
/// category or split, unless the household set the category, it's a transfer leg or it's `split`
/// already.
fn filled_category(
    disposition: &Disposition,
    had: Category,
    split: bool,
) -> (Option<i64>, Option<String>, Option<String>, &[SplitLine]) {
    let keeps = split || matches!(had.by.as_deref(), Some("user" | "transfer"));
    match disposition {
        Disposition::Mapped {
            category_id,
            reason,
        } if !keeps => (
            Some(*category_id),
            Some("import".to_owned()),
            Some(reason.clone()),
            &[],
        ),
        Disposition::Split { lines, reason } if !keeps => {
            (None, Some("import".to_owned()), Some(reason.clone()), lines)
        }
        _ => (had.id, had.by, had.reason, &[]),
    }
}

mod undo;
pub use undo::{Batch, UndoError, Undone, import_batches, undo_import};
#[cfg(test)]
#[path = "imports/undo_tests.rs"]
mod undo_tests;

#[cfg(test)]
#[path = "imports/overlap_tests.rs"]
mod overlap_tests;
#[cfg(test)]
mod tests;
