//! One-time password reset links (docs/spec/m1-ledger-import.md#login-and-profile).

use domain::login::{RESET_LIFETIME, SESSION_LIFETIME, Username};
use jiff::Timestamp;

use crate::{Error, SqlitePool, stamp};

/// Stores a new reset for the login named `username`, which works once until [`RESET_LIFETIME`]
/// from `now`, and kills any the login had before *(agent)*. `created_by` is the login that issued
/// it, or `None` for the `reset-password` CLI command.
///
/// # Errors
/// If the database fails.
pub async fn issue_password_reset(
    pool: &SqlitePool,
    username: &Username,
    token_hash: &str,
    created_by: Option<i64>,
    now: Timestamp,
) -> Result<bool, Error> {
    let username = username.as_str();
    let expires_at = stamp(now + RESET_LIFETIME);
    let mut tx = pool.begin().await?;
    let user_id = sqlx::query_scalar!(
        r#"SELECT id AS "id!: i64" FROM users WHERE username = ? AND removed_at IS NULL"#,
        username
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some(user_id) = user_id else {
        return Ok(false);
    };
    sqlx::query!("DELETE FROM password_resets WHERE user_id = ?", user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query!(
        "INSERT INTO password_resets (token_hash, user_id, created_by, expires_at)
         VALUES (?, ?, ?, ?)",
        token_hash,
        user_id,
        created_by,
        expires_at
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

/// The username a reset is for, if it still works at `now`: unused, unexpired, and its login not
/// removed.
///
/// # Errors
/// If the database fails.
pub async fn password_reset(
    pool: &SqlitePool,
    token_hash: &str,
    now: Timestamp,
) -> Result<Option<String>, Error> {
    let at = stamp(now);
    let username = sqlx::query_scalar!(
        "SELECT users.username FROM password_resets JOIN users ON users.id = password_resets.user_id
         WHERE token_hash = ? AND used_at IS NULL AND expires_at > ? AND users.removed_at IS NULL",
        token_hash,
        at
    )
    .fetch_optional(pool)
    .await?;
    Ok(username)
}

/// What using a reset writes: the login's new password and the session that logs it in.
pub struct UseReset<'a> {
    pub token_hash: &'a str,
    /// The argon2id hash, in PHC string form.
    pub password_hash: &'a str,
    /// The hash of the new session's cookie token.
    pub session_token_hash: &'a str,
}

/// Uses a reset, in one transaction, if it still works at `now` (see [`password_reset`]): marks it
/// used, sets the new password, ends the login's other sessions *(agent)*, forgets its failed
/// log-ins, and logs it in with a session that lasts [`SESSION_LIFETIME`]. Returns the login's id.
///
/// # Errors
/// If the database fails.
pub async fn use_password_reset(
    pool: &SqlitePool,
    reset: &UseReset<'_>,
    now: Timestamp,
) -> Result<Option<i64>, Error> {
    let at = stamp(now);
    let expires_at = stamp(now + SESSION_LIFETIME);
    let mut tx = pool.begin().await?;
    let user_id = sqlx::query_scalar!(
        r#"UPDATE password_resets SET used_at = ?1
           WHERE token_hash = ?2 AND used_at IS NULL AND expires_at > ?1
             AND user_id IN (SELECT id FROM users WHERE removed_at IS NULL)
           RETURNING user_id AS "user_id!: i64""#,
        at,
        reset.token_hash
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some(user_id) = user_id else {
        return Ok(None);
    };
    let username = sqlx::query_scalar!(
        "UPDATE users SET password_hash = ? WHERE id = ? RETURNING username",
        reset.password_hash,
        user_id
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query!("DELETE FROM sessions WHERE user_id = ?", user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query!("DELETE FROM login_failures WHERE username = ?", username)
        .execute(&mut *tx)
        .await?;
    sqlx::query!(
        "INSERT INTO sessions (token_hash, user_id, created_at, last_seen_at, expires_at)
         VALUES (?, ?, ?, ?, ?)",
        reset.session_token_hash,
        user_id,
        at,
        at,
        expires_at
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Some(user_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::set_up;

    fn at(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn sam() -> Username {
        Username::new("sam").unwrap()
    }

    fn reset(token_hash: &str) -> UseReset<'_> {
        UseReset {
            token_hash,
            password_hash: "$argon2id$new",
            session_token_hash: "fresh",
        }
    }

    async fn resets(pool: &SqlitePool) -> Vec<(String, Option<i64>, String, Option<String>)> {
        sqlx::query_as(
            "SELECT token_hash, created_by, expires_at, used_at FROM password_resets ORDER BY token_hash",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn issues_a_reset_for_a_known_login_that_lasts_a_day() {
        let (_dir, pool) = set_up().await;
        let now = at("2026-10-28T10:00:00Z");
        assert!(
            issue_password_reset(&pool, &sam(), "r1", None, now)
                .await
                .unwrap()
        );
        assert_eq!(
            resets(&pool).await,
            [(
                "r1".to_owned(),
                None,
                "2026-10-29T10:00:00.000Z".to_owned(),
                None
            )]
        );
        assert_eq!(
            password_reset(&pool, "r1", now).await.unwrap().as_deref(),
            Some("sam")
        );
    }

    #[tokio::test]
    async fn an_unknown_or_removed_login_gets_no_reset() {
        let (_dir, pool) = set_up().await;
        let now = at("2026-10-28T10:00:00Z");
        let alex = Username::new("alex").unwrap();
        assert!(
            !issue_password_reset(&pool, &alex, "r1", None, now)
                .await
                .unwrap()
        );
        sqlx::query("UPDATE users SET removed_at = '2026-10-28T09:30:00.000Z'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            !issue_password_reset(&pool, &sam(), "r1", None, now)
                .await
                .unwrap()
        );
        assert_eq!(resets(&pool).await, []);
    }

    #[tokio::test]
    async fn a_new_reset_kills_the_older_ones() {
        let (_dir, pool) = set_up().await;
        let now = at("2026-10-28T10:00:00Z");
        issue_password_reset(&pool, &sam(), "r1", None, now)
            .await
            .unwrap();
        issue_password_reset(&pool, &sam(), "r2", None, now)
            .await
            .unwrap();
        assert_eq!(password_reset(&pool, "r1", now).await.unwrap(), None);
        assert!(password_reset(&pool, "r2", now).await.unwrap().is_some());
        assert_eq!(
            use_password_reset(&pool, &reset("r1"), now).await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn a_reset_stops_working_after_a_day() {
        let (_dir, pool) = set_up().await;
        issue_password_reset(&pool, &sam(), "r1", None, at("2026-10-28T10:00:00Z"))
            .await
            .unwrap();
        let last = at("2026-10-29T09:59:59Z");
        let expired = at("2026-10-29T10:00:00Z");
        assert!(password_reset(&pool, "r1", last).await.unwrap().is_some());
        assert_eq!(password_reset(&pool, "r1", expired).await.unwrap(), None);
        assert_eq!(
            use_password_reset(&pool, &reset("r1"), expired)
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn using_a_reset_sets_the_password_and_logs_in_once() {
        let (_dir, pool) = set_up().await;
        let now = at("2026-10-28T10:00:00Z");
        crate::record_failure(&pool, "sam", now).await.unwrap();
        issue_password_reset(&pool, &sam(), "r1", None, now)
            .await
            .unwrap();

        let user_id = use_password_reset(&pool, &reset("r1"), now)
            .await
            .unwrap()
            .unwrap();
        let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(hash, "$argon2id$new");
        // The setup session is gone; only the reset's own is left.
        let sessions: Vec<String> = sqlx::query_scalar("SELECT token_hash FROM sessions")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(sessions, ["fresh"]);
        assert_eq!(crate::recent_failures(&pool, "sam").await.unwrap(), []);
        assert_eq!(
            resets(&pool).await[0].3.as_deref(),
            Some("2026-10-28T10:00:00.000Z")
        );

        assert_eq!(password_reset(&pool, "r1", now).await.unwrap(), None);
        let again = UseReset {
            session_token_hash: "another",
            ..reset("r1")
        };
        assert_eq!(use_password_reset(&pool, &again, now).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_removed_logins_reset_stops_working() {
        let (_dir, pool) = set_up().await;
        let now = at("2026-10-28T10:00:00Z");
        issue_password_reset(&pool, &sam(), "r1", None, now)
            .await
            .unwrap();
        sqlx::query("UPDATE users SET removed_at = '2026-10-28T10:30:00.000Z'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(password_reset(&pool, "r1", now).await.unwrap(), None);
        assert_eq!(
            use_password_reset(&pool, &reset("r1"), now).await.unwrap(),
            None
        );
    }
}
