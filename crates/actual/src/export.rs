//! Reading an unpacked export: its database opened read-only, its raw tables, never its views
//! (views can be missing or stale in older exports).

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use domain::Date;
use serde::Deserialize;
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::{ConnectOptions, Connection};

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
    /// [`ReadError::NotAnExport`] if a table the importer reads is missing, or lacks a column it
    /// can't do without; [`ReadError::Unreadable`] for a value it can't read.
    pub async fn summary(&mut self) -> Result<Summary, ReadError> {
        let accounts = self.accounts().await?;
        let live_accounts: Vec<&str> = accounts
            .iter()
            .filter(|account| !account.tombstone)
            .map(|account| account.id.as_str())
            .collect();
        let transactions = self.transactions().await?;
        let by_id: HashMap<&str, &RawTransaction> = transactions
            .iter()
            .map(|transaction| (transaction.id.as_str(), transaction))
            .collect();
        let alive = |transaction: &RawTransaction| {
            let parent_alive = || {
                transaction
                    .parent_id
                    .as_deref()
                    .and_then(|id| by_id.get(id))
                    .is_some_and(|parent| !parent.tombstone)
            };
            !transaction.tombstone
                && transaction.date.is_some()
                && transaction
                    .account
                    .as_deref()
                    .is_some_and(|id| live_accounts.contains(&id))
                && (!transaction.is_child || parent_alive())
        };
        let mut count = 0;
        let mut dates: Option<(Date, Date)> = None;
        for transaction in transactions.iter().filter(|t| alive(t)) {
            if !transaction.is_child {
                count += 1;
            }
            if let Some(date) = transaction.date {
                let date = parse_date(date)?;
                dates = Some(dates.map_or((date, date), |(first, last)| {
                    (first.min(date), last.max(date))
                }));
            }
        }
        Ok(Summary {
            budget_name: self.budget_name.clone(),
            accounts: live_accounts.len() as u64,
            transactions: count,
            categories: self.live_categories().await?,
            dates,
        })
    }

    async fn accounts(&mut self) -> Result<Vec<RawAccount>, ReadError> {
        let sql = select(&mut self.db, "accounts", ACCOUNT_COLUMNS).await?;
        let rows: Vec<(String, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .fetch_all(&mut self.db)
            .await
            .map_err(unreadable("an account"))?;
        Ok(rows
            .into_iter()
            .map(|(id, tombstone)| RawAccount {
                id,
                tombstone: tombstone != 0,
            })
            .collect())
    }

    async fn transactions(&mut self) -> Result<Vec<RawTransaction>, ReadError> {
        type Row = (
            String,
            Option<String>,
            Option<i64>,
            i64,
            Option<String>,
            i64,
        );
        let sql = select(&mut self.db, "transactions", TRANSACTION_COLUMNS).await?;
        let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .fetch_all(&mut self.db)
            .await
            .map_err(unreadable("a transaction"))?;
        Ok(rows
            .into_iter()
            .map(
                |(id, account, date, is_child, parent_id, tombstone)| RawTransaction {
                    id,
                    account,
                    date,
                    is_child: is_child != 0,
                    parent_id,
                    tombstone: tombstone != 0,
                },
            )
            .collect())
    }

    async fn live_categories(&mut self) -> Result<u64, ReadError> {
        let sql = select(&mut self.db, "categories", CATEGORY_COLUMNS).await?;
        let rows: Vec<(String, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .fetch_all(&mut self.db)
            .await
            .map_err(unreadable("a category"))?;
        Ok(rows.iter().filter(|(_, tombstone)| *tombstone == 0).count() as u64)
    }

    /// Closes the database.
    pub async fn close(self) {
        // Nothing was written, so there's nothing a failed close could lose.
        let _ = self.db.close().await;
    }
}

struct RawAccount {
    id: String,
    tombstone: bool,
}

struct RawTransaction {
    id: String,
    account: Option<String>,
    /// `YYYYMMDD`; Actual leaves out rows without one.
    date: Option<i64>,
    is_child: bool,
    parent_id: Option<String>,
    tombstone: bool,
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
}

const ACCOUNT_COLUMNS: &[Column] = &[
    Column {
        name: "id",
        kind: Kind::Required,
    },
    Column {
        name: "tombstone",
        kind: Kind::Flag,
    },
];

const TRANSACTION_COLUMNS: &[Column] = &[
    Column {
        name: "id",
        kind: Kind::Required,
    },
    Column {
        name: "acct",
        kind: Kind::Required,
    },
    Column {
        name: "date",
        kind: Kind::Required,
    },
    Column {
        name: "isChild",
        kind: Kind::Flag,
    },
    // Before Actual had `parent_id`, a split child's id was `<parent id>/<child id>` *(agent)*.
    Column {
        name: "parent_id",
        kind: Kind::Nullable(
            "CASE WHEN instr(id, '/') > 0 THEN substr(id, 1, instr(id, '/') - 1) END",
        ),
    },
    Column {
        name: "tombstone",
        kind: Kind::Flag,
    },
];

const CATEGORY_COLUMNS: &[Column] = &[
    Column {
        name: "id",
        kind: Kind::Required,
    },
    Column {
        name: "tombstone",
        kind: Kind::Flag,
    },
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
            (Kind::Nullable(missing), false) => (*missing).to_owned(),
            (Kind::Required, false) => return Err(ReadError::NotAnExport),
        });
    }
    Ok(format!("SELECT {} FROM \"{table}\"", read.join(", ")))
}

fn unreadable(what: &'static str) -> impl Fn(sqlx::Error) -> ReadError {
    move |error| ReadError::Unreadable(format!("{what} ({error})"))
}

/// Actual's integer `YYYYMMDD` date.
fn parse_date(date: i64) -> Result<Date, ReadError> {
    let no_such_day = || ReadError::Unreadable(format!("a transaction dated {date}"));
    let year = i16::try_from(date / 10_000).map_err(|_| no_such_day())?;
    let month = i8::try_from(date / 100 % 100).map_err(|_| no_such_day())?;
    let day = i8::try_from(date % 100).map_err(|_| no_such_day())?;
    Date::from_ymd(year, month, day).map_err(|_| no_such_day())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn opens_the_database_read_only_and_leaves_it_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let zip = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/actual-export.zip");
        crate::unpack(&zip, dir.path()).unwrap();
        let before = std::fs::read(dir.path().join(DB_FILE)).unwrap();

        let mut export = Export::open(dir.path()).await.unwrap();
        export.summary().await.unwrap();
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

    #[test]
    fn reads_actual_dates() {
        assert_eq!(parse_date(20_260_131).unwrap().to_string(), "2026-01-31");
        for bad in [20_260_231, 20_261_301, 0, -20_260_101, 99_999_999_999] {
            let error = parse_date(bad).unwrap_err();
            assert!(matches!(error, ReadError::Unreadable(_)), "{bad}: {error}");
        }
    }
}
