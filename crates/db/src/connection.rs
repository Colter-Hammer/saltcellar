//! The household's one bank connection (docs/spec/m2-bank-data.md#the-connection). The access URL
//! is stored sealed: this module only ever sees the sealed bytes, never the URL or the key.

use jiff::Timestamp;

use crate::{Error, SqlitePool, parse_stamp, stamp};

/// The provider's name in the `provider` column.
const PROVIDER: &str = "simplefin";
/// What the connection is called on screen *(agent)*.
const LABEL: &str = "SimpleFIN Bridge";

/// Where the connection stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Ok,
    /// The Bridge turned the access URL down (403): a new setup token is the fix.
    TokenRevoked,
    /// The Bridge subscription is unpaid (402).
    Unpaid,
    /// `BUDGET_SECRET_KEY` is missing or can't open the stored secret.
    KeyMismatch,
}

impl ConnectionStatus {
    fn as_str(self) -> &'static str {
        match self {
            ConnectionStatus::Ok => "ok",
            ConnectionStatus::TokenRevoked => "token_revoked",
            ConnectionStatus::Unpaid => "unpaid",
            ConnectionStatus::KeyMismatch => "key_mismatch",
        }
    }

    fn parse(text: &str) -> Result<ConnectionStatus, Error> {
        match text {
            "ok" => Ok(ConnectionStatus::Ok),
            "token_revoked" => Ok(ConnectionStatus::TokenRevoked),
            "unpaid" => Ok(ConnectionStatus::Unpaid),
            "key_mismatch" => Ok(ConnectionStatus::KeyMismatch),
            other => Err(Error::Unreadable {
                what: "a connection status",
                value: other.to_owned(),
            }),
        }
    }
}

/// The connection, without its secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub id: i64,
    pub label: String,
    pub status: ConnectionStatus,
    pub last_sync_at: Option<Timestamp>,
    pub last_error: Option<String>,
    pub paused_at: Option<Timestamp>,
}

/// The connection, if there is one.
///
/// # Errors
/// If the database fails.
pub async fn connection(pool: &SqlitePool) -> Result<Option<Connection>, Error> {
    let row = sqlx::query!(
        r#"SELECT id AS "id!: i64", label, status, last_sync_at, last_error, paused_at
           FROM connections ORDER BY id LIMIT 1"#
    )
    .fetch_optional(pool)
    .await?;
    row.map(|row| {
        Ok(Connection {
            id: row.id,
            label: row.label,
            status: ConnectionStatus::parse(&row.status)?,
            last_sync_at: row.last_sync_at.map(parse_stamp).transpose()?,
            last_error: row.last_error,
            paused_at: row.paused_at.map(parse_stamp).transpose()?,
        })
    })
    .transpose()
}

/// The connection's id and its sealed secret, for the code that holds the key.
///
/// # Errors
/// If the database fails.
pub async fn connection_secret(pool: &SqlitePool) -> Result<Option<(i64, Vec<u8>)>, Error> {
    let row = sqlx::query!(
        r#"SELECT id AS "id!: i64", secret_encrypted FROM connections ORDER BY id LIMIT 1"#
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| (row.id, row.secret_encrypted)))
}

/// Stores a newly claimed, sealed secret: on the household's connection if it has one (replacing
/// the secret, clearing its error and pause, and keeping its accounts), else as a new one.
/// Resolves to the connection's id.
///
/// # Errors
/// If the database fails.
pub async fn save_connection(pool: &SqlitePool, sealed: &[u8]) -> Result<i64, Error> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let existing =
        sqlx::query_scalar!(r#"SELECT id AS "id!: i64" FROM connections ORDER BY id LIMIT 1"#)
            .fetch_optional(&mut *tx)
            .await?;
    let id = if let Some(id) = existing {
        sqlx::query!(
            "UPDATE connections
             SET secret_encrypted = ?, status = 'ok', last_error = NULL, paused_at = NULL
             WHERE id = ?",
            sealed,
            id
        )
        .execute(&mut *tx)
        .await?;
        id
    } else {
        sqlx::query_scalar!(
            r#"INSERT INTO connections (provider, label, secret_encrypted)
               VALUES (?, ?, ?) RETURNING id AS "id!: i64""#,
            PROVIDER,
            LABEL,
            sealed
        )
        .fetch_one(&mut *tx)
        .await?
    };
    tx.commit().await?;
    Ok(id)
}

/// Sets the connection's status; pausing statuses record when, `ok` clears it. Does nothing
/// without a connection or when the status is already `status`.
///
/// # Errors
/// If the database fails.
pub async fn set_connection_status(
    pool: &SqlitePool,
    status: ConnectionStatus,
    now: Timestamp,
) -> Result<(), Error> {
    let status_text = status.as_str();
    let paused_at = (status != ConnectionStatus::Ok).then(|| stamp(now));
    sqlx::query!(
        "UPDATE connections
         SET status = ?, paused_at = CASE WHEN ? = 'ok' THEN NULL ELSE COALESCE(paused_at, ?) END
         WHERE status != ?",
        status_text,
        status_text,
        paused_at,
        status_text
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Deletes the connection and with it the secret. Its accounts stay, with their rows, and become
/// manual accounts. Resolves to whether there was a connection.
///
/// # Errors
/// If the database fails.
pub async fn disconnect(pool: &SqlitePool) -> Result<bool, Error> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query!("UPDATE accounts SET connection_id = NULL WHERE connection_id IS NOT NULL")
        .execute(&mut *tx)
        .await?;
    let deleted = sqlx::query!("DELETE FROM connections")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(deleted.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::migrated;

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    #[tokio::test]
    async fn there_is_no_connection_until_one_is_saved() {
        let (_dir, pool) = migrated().await;
        assert_eq!(connection(&pool).await.unwrap(), None);
        assert_eq!(connection_secret(&pool).await.unwrap(), None);
        assert!(!disconnect(&pool).await.unwrap());
    }

    #[tokio::test]
    async fn saving_twice_replaces_the_secret_on_the_same_connection() {
        let (_dir, pool) = migrated().await;
        let first = save_connection(&pool, b"sealed-one").await.unwrap();
        set_connection_status(
            &pool,
            ConnectionStatus::TokenRevoked,
            at("2026-10-28T09:00:00Z"),
        )
        .await
        .unwrap();
        let paused = connection(&pool).await.unwrap().unwrap();
        assert_eq!(paused.status, ConnectionStatus::TokenRevoked);
        assert_eq!(paused.paused_at, Some(at("2026-10-28T09:00:00Z")));

        let second = save_connection(&pool, b"sealed-two").await.unwrap();
        assert_eq!(first, second);
        let again = connection(&pool).await.unwrap().unwrap();
        assert_eq!(
            (again.status, again.paused_at),
            (ConnectionStatus::Ok, None)
        );
        assert_eq!(again.label, "SimpleFIN Bridge");
        assert_eq!(
            connection_secret(&pool).await.unwrap(),
            Some((first, b"sealed-two".to_vec()))
        );
    }

    #[tokio::test]
    async fn a_status_keeps_the_time_it_was_first_paused() {
        let (_dir, pool) = migrated().await;
        save_connection(&pool, b"x").await.unwrap();
        set_connection_status(
            &pool,
            ConnectionStatus::KeyMismatch,
            at("2026-10-28T09:00:00Z"),
        )
        .await
        .unwrap();
        set_connection_status(
            &pool,
            ConnectionStatus::KeyMismatch,
            at("2026-10-29T09:00:00Z"),
        )
        .await
        .unwrap();
        let paused = connection(&pool).await.unwrap().unwrap();
        assert_eq!(paused.paused_at, Some(at("2026-10-28T09:00:00Z")));
        set_connection_status(&pool, ConnectionStatus::Ok, at("2026-10-30T09:00:00Z"))
            .await
            .unwrap();
        assert_eq!(connection(&pool).await.unwrap().unwrap().paused_at, None);
    }

    #[tokio::test]
    async fn disconnecting_deletes_the_secret_and_keeps_accounts_and_rows() {
        let (_dir, pool) = migrated().await;
        let id = save_connection(&pool, b"sealed").await.unwrap();
        sqlx::query(
            "INSERT INTO accounts (id, name, type, role, connection_id, external_account_id)
             VALUES (1, 'Demo Checking', 'checking', 'budgeted', ?, 'ACT-demo-1')",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO transactions (account_id, date, amount_minor, status, source, raw_description, dedupe_key)
             VALUES (1, '2026-10-20', -500, 'posted', 'provider', 'DEMO CAFE', 'k')",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert!(disconnect(&pool).await.unwrap());
        assert_eq!(connection_secret(&pool).await.unwrap(), None);
        let accounts: Vec<(String, Option<i64>, Option<String>)> =
            sqlx::query_as("SELECT name, connection_id, external_account_id FROM accounts")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            accounts,
            [(
                "Demo Checking".to_owned(),
                None,
                Some("ACT-demo-1".to_owned())
            )]
        );
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transactions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[tokio::test]
    async fn the_accounts_foreign_key_holds() {
        let (_dir, pool) = migrated().await;
        let insert = "INSERT INTO accounts (name, type, role, connection_id) VALUES ('A', 'cash', 'budgeted', 99)";
        assert!(sqlx::query(insert).execute(&pool).await.is_err());
    }
}
