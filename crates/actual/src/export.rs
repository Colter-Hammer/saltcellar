//! Reading an unpacked export: its database opened read-only, its raw tables, never its views
//! (views can be missing or stale in older exports).

use std::io::Read;
use std::path::Path;

use domain::{Date, actual};
use serde::Deserialize;
use sqlx::sqlite::{Sqlite, SqliteConnectOptions, SqliteConnection, SqliteRow};
use sqlx::{ConnectOptions, Connection, Row as _};

use crate::{DB_FILE, METADATA_FILE, ReadError};

/// An unpacked Actual export, open for reading. Its database is opened read-only: nothing the
/// app does can change it.
pub struct Export {
    db: SqliteConnection,
    budget_name: String,
}

/// What step 1 of the importer shows: the budget's name and how much is in it. Tombstoned rows
/// don't count, and neither do rows that are only alive in Actual's eyes through a deleted row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub budget_name: String,
    /// Live accounts, closed ones included.
    pub accounts: u64,
    /// Live transactions in live accounts. A split counts once, as its parent *(agent)*; starting
    /// balances count, being rows in Actual.
    pub transactions: u64,
    /// Live categories.
    pub categories: u64,
    /// The earliest and latest dates of those transactions; `None` when there are none.
    pub dates: Option<(Date, Date)>,
}

#[derive(Deserialize)]
struct Metadata {
    #[serde(rename = "budgetName")]
    budget_name: String,
}

impl Export {
    /// Opens the export [`crate::unpack`] wrote into `dir`.
    ///
    /// # Errors
    /// [`ReadError::NotAnExport`] if `metadata.json` has no budget name or `db.sqlite` isn't a
    /// SQLite file; [`ReadError::Io`] if the files can't be read.
    pub async fn open(dir: &Path) -> Result<Export, ReadError> {
        let mut metadata = String::new();
        std::fs::File::open(dir.join(METADATA_FILE))?
            .take(1 << 20)
            .read_to_string(&mut metadata)
            .map_err(|_| ReadError::NotAnExport)?;
        let Metadata { budget_name } =
            serde_json::from_str(&metadata).map_err(|_| ReadError::NotAnExport)?;
        let db = SqliteConnectOptions::new()
            .filename(dir.join(DB_FILE))
            .read_only(true)
            .connect()
            .await
            .map_err(|_| ReadError::NotAnExport)?;
        Ok(Export { db, budget_name })
    }

    #[must_use]
    pub fn budget_name(&self) -> &str {
        &self.budget_name
    }

    /// The budget's name, and its live accounts, transactions, categories and date range.
    ///
    /// # Errors
    /// As for [`Export::read`].
    pub async fn summary(&mut self) -> Result<Summary, ReadError> {
        let export = self.read().await?;
        Ok(Summary {
            budget_name: self.budget_name.clone(),
            accounts: export.accounts.len() as u64,
            transactions: export.transactions.len() as u64,
            categories: export.categories.len() as u64,
            dates: export.dates(),
        })
    }

    /// Every live account, category and transaction, with each account's figures and each
    /// category's use (docs/spec/m1-ledger-import.md#reading-the-export).
    ///
    /// # Errors
    /// [`ReadError::NotAnExport`] if a table the importer reads is missing, or lacks a column it
    /// can't do without; [`ReadError::Unreadable`] for a value it can't read.
    pub async fn read(&mut self) -> Result<actual::Export, ReadError> {
        let raw = actual::Raw {
            accounts: self.accounts().await?,
            groups: self.groups().await?,
            categories: self.categories().await?,
            category_mapping: self
                .mapping("category_mapping", "transferId", "a category merge")
                .await?,
            payees: self.payees().await?,
            payee_mapping: self
                .mapping("payee_mapping", "targetId", "a payee merge")
                .await?,
            transactions: self.transactions().await?,
        };
        raw.read()
            .map_err(|actual::Unreadable(what)| ReadError::Unreadable(what))
    }

    /// Every row of `table`, its `columns` in order.
    async fn rows(
        &mut self,
        table: &str,
        columns: &[Column],
        what: &'static str,
    ) -> Result<Vec<Row>, ReadError> {
        let sql = select(&mut self.db, table, columns).await?;
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_all(&mut self.db)
            .await
            .map_err(unreadable(what))?;
        Ok(rows.into_iter().map(|row| Row { row, what }).collect())
    }

    async fn accounts(&mut self) -> Result<Vec<actual::RawAccount>, ReadError> {
        let rows = self.rows("accounts", ACCOUNT_COLUMNS, "an account").await?;
        rows.iter()
            .map(|row| {
                Ok(actual::RawAccount {
                    id: row.get(0)?,
                    name: row.get::<Option<String>>(1)?.unwrap_or_default(),
                    off_budget: row.flag(2)?,
                    closed: row.flag(3)?,
                    bank_id: row.get(4)?,
                    sort_order: row.get(5)?,
                    tombstone: row.flag(6)?,
                })
            })
            .collect()
    }

    async fn groups(&mut self) -> Result<Vec<actual::RawGroup>, ReadError> {
        let rows = self
            .rows("category_groups", GROUP_COLUMNS, "a category group")
            .await?;
        rows.iter()
            .map(|row| {
                Ok(actual::RawGroup {
                    id: row.get(0)?,
                    name: row.get::<Option<String>>(1)?.unwrap_or_default(),
                    income: row.flag(2)?,
                    sort_order: row.get(3)?,
                    tombstone: row.flag(4)?,
                })
            })
            .collect()
    }

    async fn categories(&mut self) -> Result<Vec<actual::RawCategory>, ReadError> {
        let rows = self
            .rows("categories", CATEGORY_COLUMNS, "a category")
            .await?;
        rows.iter()
            .map(|row| {
                Ok(actual::RawCategory {
                    id: row.get(0)?,
                    name: row.get::<Option<String>>(1)?.unwrap_or_default(),
                    group: row.get(2)?,
                    sort_order: row.get(3)?,
                    tombstone: row.flag(4)?,
                })
            })
            .collect()
    }

    async fn payees(&mut self) -> Result<Vec<actual::RawPayee>, ReadError> {
        let rows = self.rows("payees", PAYEE_COLUMNS, "a payee").await?;
        rows.iter()
            .map(|row| {
                Ok(actual::RawPayee {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    transfer_account: row.get(2)?,
                    tombstone: row.flag(3)?,
                })
            })
            .collect()
    }

    /// A merge mapping as `(id, target)` pairs; a row without a target is left out.
    async fn mapping(
        &mut self,
        table: &str,
        target: &'static str,
        what: &'static str,
    ) -> Result<Vec<(String, String)>, ReadError> {
        let columns = [required("id"), required(target)];
        let rows = self.rows(table, &columns, what).await?;
        let mut pairs = Vec::with_capacity(rows.len());
        for row in &rows {
            if let Some(to) = row.get::<Option<String>>(1)? {
                pairs.push((row.get(0)?, to));
            }
        }
        Ok(pairs)
    }

    async fn transactions(&mut self) -> Result<Vec<actual::RawTransaction>, ReadError> {
        let rows = self
            .rows("transactions", TRANSACTION_COLUMNS, "a transaction")
            .await?;
        rows.iter()
            .map(|row| {
                Ok(actual::RawTransaction {
                    id: row.get(0)?,
                    account: row.get(1)?,
                    date: row.get(2)?,
                    is_child: row.flag(3)?,
                    parent_id: row.get(4)?,
                    tombstone: row.flag(5)?,
                    category: row.get(6)?,
                    amount: row.get(7)?,
                    payee: row.get(8)?,
                    imported_description: row.get(9)?,
                    notes: row.get(10)?,
                    financial_id: row.get(11)?,
                    is_parent: row.flag(12)?,
                    transferred_id: row.get(13)?,
                    starting_balance: row.flag(14)?,
                })
            })
            .collect()
    }

    /// Closes the database.
    pub async fn close(self) {
        // Nothing was written, so there's nothing a failed close could lose.
        let _ = self.db.close().await;
    }
}

/// A row of a raw table, read by column position, naming what it is when a value won't read.
struct Row {
    row: SqliteRow,
    what: &'static str,
}

impl Row {
    fn get<'r, T>(&'r self, index: usize) -> Result<T, ReadError>
    where
        T: sqlx::Decode<'r, Sqlite> + sqlx::Type<Sqlite>,
    {
        self.row.try_get(index).map_err(unreadable(self.what))
    }

    /// A 0/1 flag; [`select`] has made NULL or missing 0 already.
    fn flag(&self, index: usize) -> Result<bool, ReadError> {
        Ok(self.get::<i64>(index)? != 0)
    }
}

/// A column the importer reads, and what to read when an older export lacks it.
struct Column {
    name: &'static str,
    kind: Kind,
}

enum Kind {
    /// Every export has it; one without isn't an Actual export.
    Required,
    /// A 0/1 flag: NULL or missing reads as 0.
    Flag,
    /// May be NULL; missing reads as this SQL expression.
    Nullable(&'static str),
    /// A sort position, as a real number: NULL or missing reads as 0.
    Order,
}

const fn required(name: &'static str) -> Column {
    Column {
        name,
        kind: Kind::Required,
    }
}

const fn flag(name: &'static str) -> Column {
    Column {
        name,
        kind: Kind::Flag,
    }
}

/// May be NULL, and reads as NULL when an older export lacks it.
const fn nullable(name: &'static str) -> Column {
    Column {
        name,
        kind: Kind::Nullable("NULL"),
    }
}

const fn order(name: &'static str) -> Column {
    Column {
        name,
        kind: Kind::Order,
    }
}

const ACCOUNT_COLUMNS: &[Column] = &[
    required("id"),
    required("name"),
    flag("offbudget"),
    flag("closed"),
    // Actual's bank-sync account id.
    nullable("account_id"),
    order("sort_order"),
    flag("tombstone"),
];

const GROUP_COLUMNS: &[Column] = &[
    required("id"),
    required("name"),
    flag("is_income"),
    order("sort_order"),
    flag("tombstone"),
];

const CATEGORY_COLUMNS: &[Column] = &[
    required("id"),
    required("name"),
    required("cat_group"),
    order("sort_order"),
    flag("tombstone"),
];

const PAYEE_COLUMNS: &[Column] = &[
    required("id"),
    required("name"),
    nullable("transfer_acct"),
    flag("tombstone"),
];

const TRANSACTION_COLUMNS: &[Column] = &[
    required("id"),
    required("acct"),
    required("date"),
    flag("isChild"),
    // Before Actual had `parent_id`, a split child's id was `<parent id>/<child id>` *(agent)*.
    Column {
        name: "parent_id",
        kind: Kind::Nullable(
            "CASE WHEN instr(id, '/') > 0 THEN substr(id, 1, instr(id, '/') - 1) END",
        ),
    },
    flag("tombstone"),
    required("category"),
    required("amount"),
    // A payee id.
    required("description"),
    nullable("imported_description"),
    nullable("notes"),
    nullable("financial_id"),
    flag("isParent"),
    nullable("transferred_id"),
    flag("starting_balance_flag"),
];

/// A `SELECT` of `columns` from the raw `table`, in that order, filling in each column the export
/// lacks (checked with `PRAGMA table_info`). Built only from the constant names above, so it's
/// safe to run as SQL.
async fn select(
    db: &mut SqliteConnection,
    table: &str,
    columns: &[Column],
) -> Result<String, ReadError> {
    let present: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info(?)")
        .bind(table)
        .fetch_all(&mut *db)
        .await
        .map_err(|_| ReadError::NotAnExport)?;
    if present.is_empty() {
        return Err(ReadError::NotAnExport);
    }
    let mut read = Vec::with_capacity(columns.len());
    for Column { name, kind } in columns {
        let has = present.iter().any(|column| column == name);
        read.push(match (kind, has) {
            (Kind::Required | Kind::Nullable(_), true) => format!("\"{name}\""),
            (Kind::Flag, true) => format!("COALESCE(\"{name}\", 0)"),
            (Kind::Flag, false) => "0".to_owned(),
            (Kind::Order, true) => format!("CAST(COALESCE(\"{name}\", 0) AS REAL)"),
            (Kind::Order, false) => "0.0".to_owned(),
            (Kind::Nullable(missing), false) => (*missing).to_owned(),
            (Kind::Required, false) => return Err(ReadError::NotAnExport),
        });
    }
    Ok(format!("SELECT {} FROM \"{table}\"", read.join(", ")))
}

fn unreadable(what: &'static str) -> impl Fn(sqlx::Error) -> ReadError {
    move |error| ReadError::Unreadable(format!("{what} ({error})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn unpacked() -> (tempfile::TempDir, Export) {
        let dir = tempfile::tempdir().unwrap();
        let zip = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/actual-export.zip");
        crate::unpack(&zip, dir.path()).unwrap();
        let export = Export::open(dir.path()).await.unwrap();
        (dir, export)
    }

    #[tokio::test]
    async fn opens_the_database_read_only_and_leaves_it_unchanged() {
        let (dir, mut export) = unpacked().await;
        let before = std::fs::read(dir.path().join(DB_FILE)).unwrap();

        export.read().await.unwrap();
        let error = sqlx::query("CREATE TABLE app_was_here (id INTEGER)")
            .execute(&mut export.db)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("readonly"), "{error}");
        let error = sqlx::query("UPDATE accounts SET tombstone = 1")
            .execute(&mut export.db)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("readonly"), "{error}");
        export.close().await;

        assert_eq!(std::fs::read(dir.path().join(DB_FILE)).unwrap(), before);
    }

    #[tokio::test]
    async fn a_database_that_isnt_sqlite_is_not_an_export() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(DB_FILE), b"not a database at all").unwrap();
        std::fs::write(dir.path().join(METADATA_FILE), br#"{"budgetName":"Fake"}"#).unwrap();
        let outcome = match Export::open(dir.path()).await {
            Ok(mut export) => export.summary().await.map(|_| ()),
            Err(error) => Err(error),
        };
        assert!(
            matches!(outcome, Err(ReadError::NotAnExport)),
            "{outcome:?}"
        );
    }

    #[tokio::test]
    async fn metadata_without_a_budget_name_is_not_an_export() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(DB_FILE), b"").unwrap();
        std::fs::write(dir.path().join(METADATA_FILE), br#"{"id":"x"}"#).unwrap();
        let error = Export::open(dir.path()).await.err().unwrap();
        assert!(matches!(error, ReadError::NotAnExport), "{error}");
    }

    #[tokio::test]
    async fn columns_an_older_export_lacks_are_defaulted() {
        let mut db = SqliteConnectOptions::new()
            .in_memory(true)
            .connect()
            .await
            .unwrap();
        sqlx::query("CREATE TABLE accounts (id TEXT, name TEXT)")
            .execute(&mut db)
            .await
            .unwrap();
        let sql = select(&mut db, "accounts", ACCOUNT_COLUMNS).await.unwrap();
        assert_eq!(
            sql,
            r#"SELECT "id", "name", 0, 0, NULL, 0.0, 0 FROM "accounts""#
        );
        // Without a column it can't do without, or the table, it isn't an export.
        let error = select(&mut db, "accounts", TRANSACTION_COLUMNS)
            .await
            .unwrap_err();
        assert!(matches!(error, ReadError::NotAnExport), "{error}");
        let error = select(&mut db, "payees", PAYEE_COLUMNS).await.unwrap_err();
        assert!(matches!(error, ReadError::NotAnExport), "{error}");
    }

    #[tokio::test]
    async fn a_value_of_the_wrong_kind_is_unreadable() {
        let (_dir, mut export) = unpacked().await;
        // The connection is read-only, so make the bad value in a temporary view of its own.
        sqlx::query(
            "CREATE TEMP TABLE payees (id TEXT, name BLOB, transfer_acct TEXT, tombstone INTEGER)",
        )
        .execute(&mut export.db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO temp.payees VALUES ('p', x'00ff', NULL, 0)")
            .execute(&mut export.db)
            .await
            .unwrap();
        let error = export.read().await.unwrap_err();
        assert!(
            matches!(&error, ReadError::Unreadable(what) if what.starts_with("a payee (")),
            "{error}"
        );
        export.close().await;
    }
}
