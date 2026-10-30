//! The SQLite database: one file in the data dir, in WAL mode, owned by the app alone.

mod accounts;
mod auth;
mod categories;
mod household;
mod imports;
mod profile;
mod reset;

use std::path::Path;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

pub use accounts::{Account, accounts};
pub use auth::{
    FinishSetupError, NewSetup, PasswordLogin, SessionLogin, end_session, finish_setup, log_in,
    password_login, recent_failures, record_failure, use_session,
};
pub use categories::{
    Category, CategoryError, Group, add_category, add_group, category_groups, delete_category,
    delete_group, set_order, update_category, update_group, use_starter_categories,
};
pub use household::{Household, household};
pub use imports::actual_import_in_place;
pub use profile::{
    Profile, ProfileChange, end_all_sessions, password_hash, profile, set_password, update_profile,
};
pub use reset::{UseReset, issue_password_reset, password_reset, use_password_reset};
pub use sqlx::SqlitePool;

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
                "categories",
                "category_groups",
                "household",
                "login_failures",
                "password_resets",
                "payee_aliases",
                "payees",
                "sessions",
                "settings",
                "tags",
                "users"
            ]
        );
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
