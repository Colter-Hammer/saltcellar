//! The SQLite database: one file in the data dir, in WAL mode, owned by the app alone.

mod accounts;
mod auth;
mod categories;
mod connection;
mod connection_accounts;
mod household;
mod imports;
mod manual;
mod profile;
mod reset;
mod sync;
mod transactions;
mod undecided;

use std::path::Path;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

pub use accounts::{
    Account, AccountBalance, AccountChange, AccountError, NewAccount, account_balance,
    account_balances, accounts, add_manual_snapshot, create_manual_account, set_account_order,
    update_account,
};
pub use auth::{
    FinishSetupError, NewSetup, PasswordLogin, SessionLogin, end_session, finish_setup, log_in,
    password_login, recent_failures, record_failure, use_session,
};
pub use categories::{
    Category, CategoryError, Group, add_category, add_group, category_groups, delete_category,
    delete_group, set_order, update_category, update_group, use_starter_categories,
};
pub use connection::{
    Connection, ConnectionStatus, connection, connection_secret, disconnect, save_connection,
    set_connection_status,
};
pub use connection_accounts::{
    ConnectionAccount, FeedDecision, Recorded, ReportedAccount, Track, TrackError,
    connection_accounts, ignore_connection_account, record_connection_accounts,
    track_connection_account,
};
pub use household::{Household, household};
pub use imports::{
    ActualCommit, Batch, Committed, CommittedAccount, ImportError, UndoError, Undone,
    actual_import_in_place, commit_actual_import, import_batches, undo_import,
};
pub use manual::{
    Author, ManualError, Payee, add_manual_transaction, delete_manual_transaction, payees,
    update_manual_transaction,
};
pub use profile::{
    Profile, ProfileChange, end_all_sessions, password_hash, profile, set_palette, set_password,
    update_profile,
};
pub use reset::{UseReset, issue_password_reset, password_reset, use_password_reset};
pub use sqlx::SqlitePool;
pub use sync::{
    AccountResult, Applied, FeedLink, RunEnd, SyncRun, SyncTrigger, apply_synced_rows,
    finish_sync_run, latest_sync_run, mark_backfilled, start_sync_run, tracked_feed_accounts,
};
pub use transactions::{
    BatchRef, Filter, SplitLine, Tag, TransactionDetail, TransactionPage, TransactionRow, tags,
    transaction, transactions,
};
pub use undecided::{
    DecideError, Decision, UndecidedGroup, decide_group, decide_row, undecided_groups,
};

/// The database file's name inside `BUDGET_DATA_DIR`.
pub const FILE_NAME: &str = "budget.sqlite";

/// Every migration in `migrations/`, embedded at build time.
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration failed: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    /// A stored value the app can't read back, e.g. a currency no longer on the list.
    #[error("the database holds {what} the app can't read: {value:?}")]
    Unreadable { what: &'static str, value: String },
}

/// How timestamps are stored: UTC to the millisecond, the same text SQLite's
/// `strftime('%Y-%m-%dT%H:%M:%fZ','now')` writes, so the text sorts in time order.
fn stamp(at: jiff::Timestamp) -> String {
    at.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

fn parse_stamp(text: String) -> Result<jiff::Timestamp, Error> {
    text.parse().map_err(|_| Error::Unreadable {
        what: "a timestamp",
        value: text,
    })
}

/// Opens the database at `path`, creating the file if it's missing.
///
/// Transactions that write start with `BEGIN IMMEDIATE`: a plain `BEGIN` that reads first and
/// then writes gets "database is locked" at once, without waiting out the busy timeout, if
/// another connection wrote in between.
///
/// # Errors
/// If the file can't be opened or isn't a SQLite database.
pub async fn connect(path: &Path) -> Result<SqlitePool, Error> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new().connect_with(options).await?;
    Ok(pool)
}

/// Applies every migration not yet applied. Fails if an applied one has since changed.
///
/// # Errors
/// If a migration fails or doesn't match the one applied earlier.
pub async fn migrate(pool: &SqlitePool) -> Result<(), Error> {
    MIGRATOR.run(pool).await?;
    Ok(())
}

/// Whether the database answers a query.
///
/// # Errors
/// If it doesn't.
pub async fn ping(pool: &SqlitePool) -> Result<(), Error> {
    sqlx::query_scalar!(r#"SELECT 1 AS "one!: i64""#)
        .fetch_one(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn fresh() -> (tempfile::TempDir, SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let pool = connect(&dir.path().join(FILE_NAME)).await.unwrap();
        (dir, pool)
    }

    /// A fresh database with every migration applied.
    pub(crate) async fn migrated() -> (tempfile::TempDir, SqlitePool) {
        let (dir, pool) = fresh().await;
        migrate(&pool).await.unwrap();
        (dir, pool)
    }

    /// A finished setup: `sam`'s login and the session `abc123`, from 2026-10-28 09:00.
    pub(crate) async fn set_up() -> (tempfile::TempDir, SqlitePool) {
        let (dir, pool) = migrated().await;
        let timezone = "America/Chicago".parse().unwrap();
        let display_name = domain::login::DisplayName::new("Sam").unwrap();
        let username = domain::login::Username::new("sam").unwrap();
        let setup = NewSetup {
            currency: "CAD".parse().unwrap(),
            timezone: &timezone,
            display_name: &display_name,
            username: &username,
            password_hash: "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA",
            session_token_hash: "abc123",
        };
        finish_setup(&pool, &setup, "2026-10-28T09:00:00Z".parse().unwrap())
            .await
            .unwrap();
        (dir, pool)
    }

    #[tokio::test]
    async fn creates_the_file_in_wal_mode() {
        let (dir, pool) = fresh().await;
        assert!(dir.path().join(FILE_NAME).exists());
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mode, "wal");
        let fks: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(fks, 1);
    }

    #[tokio::test]
    async fn migrates_to_the_first_tables_and_is_idempotent() {
        let (_dir, pool) = fresh().await;
        migrate(&pool).await.unwrap();
        migrate(&pool).await.unwrap();
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE '\\_%' ESCAPE '\\' ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            tables,
            [
                "accounts",
                "balance_snapshots",
                "categories",
                "category_groups",
                "connection_accounts",
                "connections",
                "deleted_bank_rows",
                "household",
                "import_batch_rows",
                "import_batches",
                "login_failures",
                "password_resets",
                "payee_aliases",
                "payees",
                "sessions",
                "settings",
                "sync_account_results",
                "sync_runs",
                "tags",
                "transaction_splits",
                "transaction_tags",
                "transactions",
                "transfers",
                "users"
            ]
        );
    }

    /// Every migration before the connection's (0011) applied, then the rest: the accounts
    /// rebuild keeps every row and what hangs off it, and the key now holds.
    #[tokio::test]
    async fn the_accounts_rebuild_keeps_rows_and_what_points_at_them() {
        /// An account's id, name, in-net-worth, closed, order, provider id, opening balance and
        /// batch: the columns that carry over.
        type Carried = (
            i64,
            String,
            i64,
            i64,
            i64,
            Option<String>,
            Option<i64>,
            Option<i64>,
        );
        let (_dir, pool) = fresh().await;
        let before = sqlx::migrate::Migrator {
            migrations: std::borrow::Cow::Owned(
                MIGRATOR
                    .migrations
                    .iter()
                    .filter(|m| m.version < 11)
                    .cloned()
                    .collect(),
            ),
            ..sqlx::migrate::Migrator::DEFAULT
        };
        before.run(&pool).await.unwrap();
        for sql in [
            "INSERT INTO accounts (id, name, institution, type, role, has_transactions, in_net_worth,
               closed, sort_order, external_account_id, opening_balance_minor, opening_date,
               balance_includes_pending, created_by_batch_id)
             VALUES (7, 'Demo Checking', 'Demo Bank', 'checking', 'budgeted', 1, 0, 1, 3, 'ACT-demo-1',
               10000, '2026-09-30', 0, 5)",
            "INSERT INTO accounts (id, name, type, role) VALUES (8, 'Demo Cash', 'cash', 'tracking')",
            "INSERT INTO transactions (account_id, date, amount_minor, raw_description, source, dedupe_key)
             VALUES (7, '2026-10-20', -500, 'DEMO CAFE', 'import', 'k')",
            "INSERT INTO balance_snapshots (account_id, as_of, balance_minor, source)
             VALUES (7, '2026-10-20', 9500, 'manual')",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }

        migrate(&pool).await.unwrap();

        let accounts: Vec<Carried> = sqlx::query_as(
            "SELECT id, name, in_net_worth, closed, sort_order, external_account_id,
                        opening_balance_minor, created_by_batch_id FROM accounts ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            accounts,
            [
                (
                    7,
                    "Demo Checking".to_owned(),
                    0,
                    1,
                    3,
                    Some("ACT-demo-1".to_owned()),
                    Some(10000),
                    Some(5)
                ),
                (8, "Demo Cash".to_owned(), 1, 0, 0, None, None, None),
            ]
        );
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transactions")
            .fetch_one(&pool)
            .await
            .unwrap();
        let snapshots: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM balance_snapshots")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            (rows, snapshots),
            (1, 1),
            "what pointed at accounts survived the rebuild"
        );
        let broken: Vec<(String,)> =
            sqlx::query_as("SELECT \"table\" FROM pragma_foreign_key_check")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(broken.is_empty(), "{broken:?}");
        let fks: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(fks, 1, "foreign keys are back on");
        let uniques: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_index_list('accounts') WHERE \"unique\" = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(uniques, 2, "name, and (connection_id, external_account_id)");
        let view_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM v_category_lines")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(view_rows >= 0, "the views over accounts still work");
    }

    #[tokio::test]
    async fn household_holds_one_row_only() {
        let (_dir, pool) = fresh().await;
        migrate(&pool).await.unwrap();
        let insert = "INSERT INTO household (id, currency_code, timezone) VALUES (?, 'USD', 'America/New_York')";
        sqlx::query(insert).bind(1).execute(&pool).await.unwrap();
        assert!(sqlx::query(insert).bind(2).execute(&pool).await.is_err());
    }

    #[test]
    fn stamps_sort_as_text_in_time_order() {
        let at: jiff::Timestamp = "2026-10-28T09:05:03.04Z".parse().unwrap();
        assert_eq!(stamp(at), "2026-10-28T09:05:03.040Z");
        assert_eq!(parse_stamp(stamp(at)).unwrap(), at);
        assert!(parse_stamp("yesterday".to_owned()).is_err());
    }

    #[tokio::test]
    async fn pings() {
        let (_dir, pool) = fresh().await;
        ping(&pool).await.unwrap();
        pool.close().await;
        assert!(ping(&pool).await.is_err());
    }
}
