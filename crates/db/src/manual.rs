//! Manual entry (docs/spec/m2-bank-data.md#manual-entry): writing, editing and deleting the rows
//! typed in by hand, and the payees the entry form offers. A manual row is `source = 'manual'`,
//! posted *(agent)*, `categorized_by = 'user'`; one landing as **Transfer to…** is the only leg of
//! a confirmed transfer (`matched_by = 'manual'`) and carries no category. Until a bank row adopts
//! it (gives it an `external_id`), every field can change and it can be deleted.

use domain::manual::{Entry, Lands};

use crate::{Error, SqlitePool, stamp};

/// Who wrote it, and when.
#[derive(Debug, Clone)]
pub struct Author<'a> {
    pub user_id: i64,
    /// The login's display name, for the reason "Set by <name>", as on From Actual, undecided
    /// *(agent)*.
    pub display_name: &'a str,
    pub at: jiff::Timestamp,
}

#[derive(Debug, thiserror::Error)]
pub enum ManualError {
    #[error("there's no such account")]
    NoSuchAccount,
    /// The account holds balance snapshots only, so no transactions.
    #[error("the account holds balances only")]
    BalancesOnly,
    #[error("there's no such category")]
    NoSuchCategory,
    #[error("there's no such account to transfer with")]
    NoSuchTransferAccount,
    #[error("there's no such transaction")]
    NotFound,
    /// It came from the bank or a file, or a bank row has adopted it.
    #[error("the transaction isn't a manual one")]
    NotManual,
    #[error(transparent)]
    Db(#[from] Error),
}

impl From<sqlx::Error> for ManualError {
    fn from(error: sqlx::Error) -> Self {
        ManualError::Db(Error::Sqlx(error))
    }
}

/// Writes `entry` as a new manual row; returns its id.
///
/// # Errors
/// If the account, category or transfer account is missing or can't take it, or the database
/// fails; nothing is written then.
pub async fn add_manual_transaction(
    pool: &SqlitePool,
    entry: &Entry,
    author: &Author<'_>,
) -> Result<i64, ManualError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    check_references(&mut tx, entry).await?;
    let fields = Fields::of(&mut tx, entry, author).await?;
    let id = sqlx::query_scalar!(
        r#"INSERT INTO transactions
             (account_id, date, amount_minor, status, payee_id, raw_description, memo, category_id,
              categorized_by, categorization_reason, source, dedupe_key, created_at, updated_at,
              updated_by)
           VALUES (?, ?, ?, 'posted', ?, ?, ?, ?, 'user', ?, 'manual', ?, ?, ?, ?)
           RETURNING id AS "id!: i64""#,
        entry.account_id,
        fields.date,
        fields.amount,
        fields.payee_id,
        fields.raw_description,
        entry.memo,
        fields.category_id,
        fields.reason,
        fields.dedupe_key,
        fields.at,
        fields.at,
        author.user_id
    )
    .fetch_one(&mut *tx)
    .await?;
    write_links(&mut tx, id, entry, author).await?;
    tx.commit().await?;
    Ok(id)
}

/// Rewrites manual row `id` as `entry`: every field, its tags and its transfer.
///
/// # Errors
/// [`ManualError::NotFound`], [`ManualError::NotManual`] for a row that isn't manual or that a
/// bank row has adopted, or as [`add_manual_transaction`]; nothing changes then.
pub async fn update_manual_transaction(
    pool: &SqlitePool,
    id: i64,
    entry: &Entry,
    author: &Author<'_>,
) -> Result<(), ManualError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    check_editable(&mut tx, id).await?;
    check_references(&mut tx, entry).await?;
    let fields = Fields::of(&mut tx, entry, author).await?;
    sqlx::query!(
        r#"UPDATE transactions
           SET account_id = ?, date = ?, amount_minor = ?, payee_id = ?, raw_description = ?,
               memo = ?, category_id = ?, categorized_by = 'user', categorization_reason = ?,
               dedupe_key = ?, updated_at = ?, updated_by = ?
           WHERE id = ?"#,
        entry.account_id,
        fields.date,
        fields.amount,
        fields.payee_id,
        fields.raw_description,
        entry.memo,
        fields.category_id,
        fields.reason,
        fields.dedupe_key,
        fields.at,
        author.user_id,
        id
    )
    .execute(&mut *tx)
    .await?;
    // Its tags and transfer are written afresh. A manual row's transfer has only this leg until
    // the other account syncs, which milestone 2's later work fills in *(agent)*.
    sqlx::query!("DELETE FROM transaction_tags WHERE transaction_id = ?", id)
        .execute(&mut *tx)
        .await?;
    sqlx::query!(
        "DELETE FROM transfers WHERE from_txn_id = ?1 OR to_txn_id = ?1",
        id
    )
    .execute(&mut *tx)
    .await?;
    write_links(&mut tx, id, entry, author).await?;
    tx.commit().await?;
    Ok(())
}

/// Deletes manual row `id`, with its tags and its transfer.
///
/// # Errors
/// [`ManualError::NotFound`], [`ManualError::NotManual`] as [`update_manual_transaction`], or if
/// the database fails; nothing is deleted then.
pub async fn delete_manual_transaction(pool: &SqlitePool, id: i64) -> Result<(), ManualError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    check_editable(&mut tx, id).await?;
    // Tags and the transfer go with it (ON DELETE CASCADE).
    sqlx::query!("DELETE FROM transactions WHERE id = ?", id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// A payee the household has, for the entry form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payee {
    pub id: i64,
    pub name: String,
}

/// Every payee, by name (ignoring case).
///
/// # Errors
/// If the database fails.
pub async fn payees(pool: &SqlitePool) -> Result<Vec<Payee>, Error> {
    Ok(sqlx::query_as!(
        Payee,
        r#"SELECT id AS "id!: i64", name FROM payees ORDER BY name COLLATE NOCASE, id"#
    )
    .fetch_all(pool)
    .await?)
}

/// Whether row `id` is a manual one no bank row has adopted.
async fn check_editable(tx: &mut sqlx::SqliteConnection, id: i64) -> Result<(), ManualError> {
    let found = sqlx::query!(
        r#"SELECT source, external_id FROM transactions WHERE id = ?"#,
        id
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ManualError::NotFound)?;
    if found.source != "manual" || found.external_id.is_some() {
        return Err(ManualError::NotManual);
    }
    Ok(())
}

/// Whether the account, and the category or the other account, exist and can take the row.
async fn check_references(
    tx: &mut sqlx::SqliteConnection,
    entry: &Entry,
) -> Result<(), ManualError> {
    let has_transactions = sqlx::query_scalar!(
        r#"SELECT has_transactions AS "has_transactions!: bool" FROM accounts WHERE id = ?"#,
        entry.account_id
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ManualError::NoSuchAccount)?;
    if !has_transactions {
        return Err(ManualError::BalancesOnly);
    }
    match entry.lands {
        Lands::Category(category) => {
            sqlx::query_scalar!(
                r#"SELECT id AS "id!: i64" FROM categories WHERE id = ?"#,
                category
            )
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(ManualError::NoSuchCategory)?;
        }
        // Any other account, balances-only ones included: their side of the transfer stays empty.
        Lands::TransferWith(other) => {
            sqlx::query_scalar!(
                r#"SELECT id AS "id!: i64" FROM accounts WHERE id = ?"#,
                other
            )
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(ManualError::NoSuchTransferAccount)?;
        }
    }
    Ok(())
}

/// The stored forms of an entry's fields.
struct Fields {
    date: String,
    amount: i64,
    payee_id: Option<i64>,
    raw_description: String,
    category_id: Option<i64>,
    reason: String,
    dedupe_key: String,
    at: String,
}

impl Fields {
    /// `entry`'s fields as stored, its payee found or added by name.
    async fn of(
        tx: &mut sqlx::SqliteConnection,
        entry: &Entry,
        author: &Author<'_>,
    ) -> Result<Fields, ManualError> {
        let payee_id = match &entry.payee {
            Some(name) => Some(payee_id(tx, name).await?),
            None => None,
        };
        Ok(Fields {
            date: entry.date.to_string(),
            amount: entry.amount.minor(),
            payee_id,
            raw_description: entry.raw_description().to_owned(),
            category_id: match entry.lands {
                Lands::Category(category) => Some(category),
                Lands::TransferWith(_) => None,
            },
            reason: format!("Set by {}", author.display_name),
            dedupe_key: domain::import::dedupe_key(
                entry.account_id,
                entry.date,
                entry.amount,
                entry.raw_description(),
            ),
            at: stamp(author.at),
        })
    }
}

/// The payee called exactly `name`, added if there's none.
async fn payee_id(tx: &mut sqlx::SqliteConnection, name: &str) -> Result<i64, ManualError> {
    sqlx::query!(
        "INSERT INTO payees (name) VALUES (?) ON CONFLICT (name) DO NOTHING",
        name
    )
    .execute(&mut *tx)
    .await?;
    Ok(sqlx::query_scalar!(
        r#"SELECT id AS "id!: i64" FROM payees WHERE name = ?"#,
        name
    )
    .fetch_one(&mut *tx)
    .await?)
}

/// Row `id`'s tags and, for a transfer, its one-leg transfer: money out goes from its account to
/// the other, money in comes from the other.
async fn write_links(
    tx: &mut sqlx::SqliteConnection,
    id: i64,
    entry: &Entry,
    author: &Author<'_>,
) -> Result<(), ManualError> {
    for name in &entry.tags {
        sqlx::query!(
            "INSERT INTO tags (name) VALUES (?) ON CONFLICT (name) DO NOTHING",
            name
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "INSERT INTO transaction_tags (transaction_id, tag_id)
             SELECT ?, id FROM tags WHERE name = ?",
            id,
            name
        )
        .execute(&mut *tx)
        .await?;
    }
    if let Lands::TransferWith(other) = entry.lands {
        let out = entry.amount.minor() < 0;
        let (from_account, to_account) = if out {
            (entry.account_id, other)
        } else {
            (other, entry.account_id)
        };
        let (from_txn, to_txn) = if out {
            (Some(id), None)
        } else {
            (None, Some(id))
        };
        sqlx::query!(
            "INSERT INTO transfers
               (from_account_id, to_account_id, from_txn_id, to_txn_id, matched_by, state,
                confirmed_by)
             VALUES (?, ?, ?, ?, 'manual', 'confirmed', ?)",
            from_account,
            to_account,
            from_txn,
            to_txn,
            author.user_id
        )
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use domain::Money;
    use domain::manual::Lands;

    use super::*;
    use crate::tests::migrated;

    async fn ledger() -> (tempfile::TempDir, SqlitePool) {
        let (dir, pool) = migrated().await;
        sqlx::query(
            r"INSERT INTO users (id, username, display_name, password_hash)
                VALUES (1, 'sam', 'Sam', 'x');
              INSERT INTO accounts (id, name, type, role, has_transactions) VALUES
                (1, 'Wallet', 'cash', 'budgeted', 1), (2, 'Savings', 'savings', 'tracking', 1),
                (3, 'Fund', 'investment', 'tracking', 0);
              INSERT INTO category_groups (id, name) VALUES (1, 'Food');
              INSERT INTO categories (id, group_id, name) VALUES (1, 1, 'Groceries'), (2, 1, 'Dining out');
              INSERT INTO payees (id, name) VALUES (1, 'Corner Market');
              INSERT INTO transactions (id, account_id, date, amount_minor, raw_description, source,
                  dedupe_key, external_id)
                VALUES (50, 1, '2026-01-01', -100, 'FEE', 'import', 'k', NULL),
                       (51, 1, '2026-01-01', -100, 'ADOPTED', 'manual', 'k', 'bank-1');",
        )
        .execute(&pool)
        .await
        .unwrap();
        (dir, pool)
    }

    fn author() -> Author<'static> {
        Author {
            user_id: 1,
            display_name: "Sam",
            at: "2026-03-05T10:00:00Z".parse().unwrap(),
        }
    }

    fn entry(lands: Lands) -> Entry {
        Entry {
            account_id: 1,
            date: "2026-03-04".parse().unwrap(),
            amount: Money::from_minor(-1250),
            payee: Some("Corner Market".to_owned()),
            lands,
            memo: Some("milk".to_owned()),
            tags: vec!["trip".to_owned(), "weekly".to_owned()],
        }
    }

    #[tokio::test]
    async fn a_manual_row_is_written_with_its_category_tags_and_payee() {
        let (_dir, pool) = ledger().await;
        let id = add_manual_transaction(&pool, &entry(Lands::Category(1)), &author())
            .await
            .unwrap();
        let row = sqlx::query!(
            r#"SELECT status, source, categorized_by, categorization_reason, payee_id,
                      category_id, raw_description, dedupe_key, updated_by
               FROM transactions WHERE id = ?"#,
            id
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.status, "posted");
        assert_eq!(row.source, "manual");
        assert_eq!(row.categorized_by.as_deref(), Some("user"));
        assert_eq!(row.categorization_reason.as_deref(), Some("Set by Sam"));
        assert_eq!(row.payee_id, Some(1));
        assert_eq!(row.category_id, Some(1));
        assert_eq!(row.raw_description, "Corner Market");
        assert_eq!(
            row.dedupe_key,
            domain::import::dedupe_key(
                1,
                "2026-03-04".parse().unwrap(),
                Money::from_minor(-1250),
                "Corner Market"
            )
        );
        assert_eq!(row.updated_by, Some(1));
        let detail = crate::transaction(&pool, id).await.unwrap().unwrap();
        assert_eq!(detail.row.tags, ["trip", "weekly"]);
        assert!(detail.row.manual);
        assert_eq!(detail.memo.as_deref(), Some("milk"));
    }

    #[tokio::test]
    async fn a_new_payee_is_added_by_name() {
        let (_dir, pool) = ledger().await;
        let new = Entry {
            payee: Some("Fake Bakery".to_owned()),
            ..entry(Lands::Category(1))
        };
        add_manual_transaction(&pool, &new, &author())
            .await
            .unwrap();
        let names: Vec<String> = payees(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, ["Corner Market", "Fake Bakery"]);
    }

    #[tokio::test]
    async fn a_transfer_is_one_confirmed_leg_with_no_category() {
        let (_dir, pool) = ledger().await;
        let out = add_manual_transaction(&pool, &entry(Lands::TransferWith(2)), &author())
            .await
            .unwrap();
        let transfer = sqlx::query!(
            r#"SELECT from_account_id, to_account_id, from_txn_id, to_txn_id, matched_by, state,
                      confirmed_by
               FROM transfers"#
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((transfer.from_account_id, transfer.to_account_id), (1, 2));
        assert_eq!(
            (transfer.from_txn_id, transfer.to_txn_id),
            (Some(out), None)
        );
        assert_eq!(transfer.matched_by, "manual");
        assert_eq!(transfer.state, "confirmed");
        assert_eq!(transfer.confirmed_by, Some(1));
        let row = crate::transaction(&pool, out).await.unwrap().unwrap().row;
        assert_eq!(row.category_id, None);
        assert_eq!(row.transfer_account.as_deref(), Some("Savings"));
        assert_eq!(row.transfer_account_id, Some(2));

        // Money in comes from the other account; balances-only ones can be the other side.
        let money_in = Entry {
            amount: Money::from_minor(5000),
            ..entry(Lands::TransferWith(3))
        };
        let id = add_manual_transaction(&pool, &money_in, &author())
            .await
            .unwrap();
        let (from, to): (i64, Option<i64>) =
            sqlx::query_as("SELECT from_account_id, to_txn_id FROM transfers WHERE to_txn_id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((from, to), (3, Some(id)));
    }

    #[tokio::test]
    async fn missing_or_unfit_references_write_nothing() {
        let (_dir, pool) = ledger().await;
        let cases = [
            (
                Entry {
                    account_id: 9,
                    ..entry(Lands::Category(1))
                },
                "account",
            ),
            (
                Entry {
                    account_id: 3,
                    ..entry(Lands::Category(1))
                },
                "balances only",
            ),
            (entry(Lands::Category(9)), "category"),
            (entry(Lands::TransferWith(9)), "transfer account"),
        ];
        for (bad, what) in cases {
            let error = add_manual_transaction(&pool, &bad, &author())
                .await
                .unwrap_err();
            let fits = match what {
                "account" => matches!(error, ManualError::NoSuchAccount),
                "balances only" => matches!(error, ManualError::BalancesOnly),
                "category" => matches!(error, ManualError::NoSuchCategory),
                _ => matches!(error, ManualError::NoSuchTransferAccount),
            };
            assert!(fits, "{what}: {error:?}");
        }
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM transactions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn an_edit_rewrites_every_field_tags_and_transfer() {
        let (_dir, pool) = ledger().await;
        let id = add_manual_transaction(&pool, &entry(Lands::TransferWith(2)), &author())
            .await
            .unwrap();
        let changed = Entry {
            account_id: 2,
            date: "2026-03-01".parse().unwrap(),
            amount: Money::from_minor(700),
            payee: None,
            lands: Lands::Category(2),
            memo: None,
            tags: vec!["gift".to_owned()],
        };
        update_manual_transaction(&pool, id, &changed, &author())
            .await
            .unwrap();
        let detail = crate::transaction(&pool, id).await.unwrap().unwrap();
        assert_eq!(detail.row.account_id, 2);
        assert_eq!(detail.row.date.to_string(), "2026-03-01");
        assert_eq!(detail.row.amount, Money::from_minor(700));
        assert_eq!(detail.row.payee, None);
        assert_eq!(detail.row.raw_description, "");
        assert_eq!(detail.row.category_id, Some(2));
        assert_eq!(detail.row.transfer_account, None);
        assert_eq!(detail.row.tags, ["gift"]);
        assert_eq!(detail.memo, None);
        let transfers: i64 = sqlx::query_scalar("SELECT count(*) FROM transfers")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(transfers, 0);

        // And back to a transfer, the other way.
        update_manual_transaction(&pool, id, &entry(Lands::TransferWith(2)), &author())
            .await
            .unwrap();
        let row = crate::transaction(&pool, id).await.unwrap().unwrap().row;
        assert_eq!(row.category_id, None);
        assert_eq!(row.transfer_account.as_deref(), Some("Savings"));
    }

    #[tokio::test]
    async fn only_unadopted_manual_rows_can_be_edited_or_deleted() {
        let (_dir, pool) = ledger().await;
        let fine = entry(Lands::Category(1));
        for id in [50, 51] {
            assert!(matches!(
                update_manual_transaction(&pool, id, &fine, &author()).await,
                Err(ManualError::NotManual)
            ));
            assert!(matches!(
                delete_manual_transaction(&pool, id).await,
                Err(ManualError::NotManual)
            ));
        }
        assert!(matches!(
            delete_manual_transaction(&pool, 99).await,
            Err(ManualError::NotFound)
        ));
    }

    #[tokio::test]
    async fn deleting_takes_its_tags_and_transfer_with_it() {
        let (_dir, pool) = ledger().await;
        let id = add_manual_transaction(&pool, &entry(Lands::TransferWith(2)), &author())
            .await
            .unwrap();
        delete_manual_transaction(&pool, id).await.unwrap();
        assert_eq!(crate::transaction(&pool, id).await.unwrap(), None);
        let left: (i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM transfers), (SELECT count(*) FROM transaction_tags)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(left, (0, 0));
    }
}
