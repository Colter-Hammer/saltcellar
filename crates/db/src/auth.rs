use domain::login::{DisplayName, SESSION_LIFETIME, Username};
use domain::throttle::MAX_FAILURES;
use domain::{Currency, TimeZone};
use jiff::Timestamp;

use crate::{Error, SqlitePool, parse_stamp, stamp};

/// The key's most recent failed attempts, newest first: as many as the throttle looks at.
///
/// # Errors
/// If the database fails or holds a timestamp it can't read.
pub async fn recent_failures(pool: &SqlitePool, key: &str) -> Result<Vec<Timestamp>, Error> {
    let limit = i64::try_from(MAX_FAILURES).unwrap_or(i64::MAX);
    let rows = sqlx::query_scalar!(
        "SELECT failed_at FROM login_failures WHERE username = ? ORDER BY failed_at DESC LIMIT ?",
        key,
        limit
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(parse_stamp).collect()
}

/// Records a failed attempt for `key` at `at`.
///
/// # Errors
/// If the database fails.
pub async fn record_failure(pool: &SqlitePool, key: &str, at: Timestamp) -> Result<(), Error> {
    let at = stamp(at);
    sqlx::query!(
        "INSERT INTO login_failures (username, failed_at) VALUES (?, ?)",
        key,
        at
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// A login that logs in with a password: its id and its argon2id hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordLogin {
    pub user_id: i64,
    /// In PHC string form.
    pub password_hash: String,
}

/// The login with this username, if there is one that hasn't been removed and has a password.
///
/// # Errors
/// If the database fails.
pub async fn password_login(
    pool: &SqlitePool,
    username: &Username,
) -> Result<Option<PasswordLogin>, Error> {
    let username = username.as_str();
    let row = sqlx::query!(
        r#"SELECT id AS "id!: i64", password_hash AS "password_hash!: String" FROM users
           WHERE username = ? AND removed_at IS NULL AND password_hash IS NOT NULL"#,
        username
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| PasswordLogin {
        user_id: row.id,
        password_hash: row.password_hash,
    }))
}

/// Logs a login in: writes its new session, which lasts [`SESSION_LIFETIME`] from `now`, and
/// forgets the throttle key's failures, so earlier slips don't count towards a later pause
/// *(agent)*.
///
/// # Errors
/// If the database fails.
pub async fn log_in(
    pool: &SqlitePool,
    user_id: i64,
    session_token_hash: &str,
    throttle_key: &str,
    now: Timestamp,
) -> Result<(), Error> {
    let at = stamp(now);
    let expires_at = stamp(now + SESSION_LIFETIME);
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query!(
        "INSERT INTO sessions (token_hash, user_id, created_at, last_seen_at, expires_at)
         VALUES (?, ?, ?, ?, ?)",
        session_token_hash,
        user_id,
        at,
        at,
        expires_at
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "DELETE FROM login_failures WHERE username = ?",
        throttle_key
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Who a session belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLogin {
    pub user_id: i64,
    pub username: String,
    pub display_name: String,
}

/// The login a session belongs to, if the session is live at `now` and its login hasn't been
/// removed. Using it slides its expiry to [`SESSION_LIFETIME`] from `now`. An expired session is
/// deleted.
///
/// # Errors
/// If the database fails.
pub async fn use_session(
    pool: &SqlitePool,
    token_hash: &str,
    now: Timestamp,
) -> Result<Option<SessionLogin>, Error> {
    let at = stamp(now);
    let expires_at = stamp(now + SESSION_LIFETIME);
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let user_id = sqlx::query_scalar!(
        r#"UPDATE sessions SET last_seen_at = ?1, expires_at = ?2
           WHERE token_hash = ?3 AND expires_at > ?1
             AND user_id IN (SELECT id FROM users WHERE removed_at IS NULL)
           RETURNING user_id AS "user_id!: i64""#,
        at,
        expires_at,
        token_hash
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some(user_id) = user_id else {
        sqlx::query!(
            "DELETE FROM sessions WHERE token_hash = ? AND expires_at <= ?",
            token_hash,
            at
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(None);
    };
    let login = sqlx::query!(
        "SELECT username, display_name FROM users WHERE id = ?",
        user_id
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Some(SessionLogin {
        user_id,
        username: login.username,
        display_name: login.display_name,
    }))
}

/// Deletes a session: logging out.
///
/// # Errors
/// If the database fails.
pub async fn end_session(pool: &SqlitePool, token_hash: &str) -> Result<(), Error> {
    sqlx::query!("DELETE FROM sessions WHERE token_hash = ?", token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

/// What the setup wizard commits: the household, the first login and that login's first session.
pub struct NewSetup<'a> {
    pub currency: Currency,
    pub timezone: &'a TimeZone,
    pub display_name: &'a DisplayName,
    pub username: &'a Username,
    /// The argon2id hash, in PHC string form.
    pub password_hash: &'a str,
    /// The hash of the new session's cookie token.
    pub session_token_hash: &'a str,
}

#[derive(Debug, thiserror::Error)]
pub enum FinishSetupError {
    #[error("setup is already finished")]
    AlreadyDone,
    #[error(transparent)]
    Db(#[from] Error),
}

impl From<sqlx::Error> for FinishSetupError {
    fn from(error: sqlx::Error) -> Self {
        FinishSetupError::Db(Error::Sqlx(error))
    }
}

/// Finishes setup in one transaction: writes the household with `setup_done_at`, the first login
/// and its session, which lasts [`SESSION_LIFETIME`] from `now`.
///
/// # Errors
/// [`FinishSetupError::AlreadyDone`] if setup was already finished, which leaves everything as it
/// was; otherwise only if the database fails.
pub async fn finish_setup(
    pool: &SqlitePool,
    setup: &NewSetup<'_>,
    now: Timestamp,
) -> Result<(), FinishSetupError> {
    let currency = setup.currency.code();
    let timezone = setup.timezone.name();
    let username = setup.username.as_str();
    let display_name = setup.display_name.as_str();
    let done_at = stamp(now);
    let expires_at = stamp(now + SESSION_LIFETIME);

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let household = sqlx::query!(
        "INSERT INTO household (id, currency_code, timezone, setup_done_at) VALUES (1, ?1, ?2, ?3)
         ON CONFLICT (id) DO UPDATE SET currency_code = ?1, timezone = ?2, setup_done_at = ?3
         WHERE setup_done_at IS NULL",
        currency,
        timezone,
        done_at
    )
    .execute(&mut *tx)
    .await?;
    if household.rows_affected() == 0 {
        return Err(FinishSetupError::AlreadyDone);
    }
    let user_id = sqlx::query_scalar!(
        "INSERT INTO users (username, display_name, password_hash, created_at) VALUES (?, ?, ?, ?)
         RETURNING id",
        username,
        display_name,
        setup.password_hash,
        done_at
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query!(
        "INSERT INTO sessions (token_hash, user_id, created_at, last_seen_at, expires_at)
         VALUES (?, ?, ?, ?, ?)",
        setup.session_token_hash,
        user_id,
        done_at,
        done_at,
        expires_at
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{migrated, set_up};

    fn at(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    #[tokio::test]
    async fn failures_come_back_newest_first_per_key_and_only_as_many_as_count() {
        let (_dir, pool) = migrated().await;
        for minute in [3, 0, 5, 1, 4, 2] {
            let when = at(&format!("2026-10-28T09:0{minute}:00Z"));
            record_failure(&pool, "setup", when).await.unwrap();
        }
        record_failure(&pool, "sam", at("2026-10-28T10:00:00Z"))
            .await
            .unwrap();

        let recent = recent_failures(&pool, "setup").await.unwrap();
        let minutes: Vec<i8> = recent
            .iter()
            .map(|t| t.to_zoned(jiff::tz::TimeZone::UTC).minute())
            .collect();
        assert_eq!(minutes, [5, 4, 3, 2, 1]);
        assert_eq!(recent_failures(&pool, "nobody").await.unwrap(), []);
    }

    struct Fixture {
        timezone: TimeZone,
        display_name: DisplayName,
        username: Username,
    }

    fn fixture() -> Fixture {
        Fixture {
            timezone: "America/Chicago".parse().unwrap(),
            display_name: DisplayName::new("Sam").unwrap(),
            username: Username::new("sam").unwrap(),
        }
    }

    fn setup(f: &Fixture) -> NewSetup<'_> {
        NewSetup {
            currency: "CAD".parse().unwrap(),
            timezone: &f.timezone,
            display_name: &f.display_name,
            username: &f.username,
            password_hash: "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA",
            session_token_hash: "abc123",
        }
    }

    #[tokio::test]
    async fn finishing_setup_writes_the_household_login_and_session() {
        let (_dir, pool) = migrated().await;
        let f = fixture();
        finish_setup(&pool, &setup(&f), at("2026-10-28T09:00:00Z"))
            .await
            .unwrap();

        let household = crate::household(&pool).await.unwrap().unwrap();
        assert_eq!(household.currency.code(), "CAD");
        assert_eq!(household.timezone.name(), "America/Chicago");

        let (username, display_name, hash): (String, String, String) =
            sqlx::query_as("SELECT username, display_name, password_hash FROM users")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((username.as_str(), display_name.as_str()), ("sam", "Sam"));
        assert!(hash.starts_with("$argon2id$"));

        let (token_hash, expires_at): (String, String) =
            sqlx::query_as("SELECT token_hash, expires_at FROM sessions")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(token_hash, "abc123");
        assert_eq!(expires_at, "2026-11-27T09:00:00.000Z");
    }

    #[tokio::test]
    async fn finishing_twice_changes_nothing() {
        let (_dir, pool) = migrated().await;
        let f = fixture();
        finish_setup(&pool, &setup(&f), at("2026-10-28T09:00:00Z"))
            .await
            .unwrap();

        let other = Fixture {
            username: Username::new("alex").unwrap(),
            ..fixture()
        };
        let again = NewSetup {
            currency: "EUR".parse().unwrap(),
            session_token_hash: "def456",
            ..setup(&other)
        };
        let error = finish_setup(&pool, &again, at("2026-10-28T10:00:00Z"))
            .await
            .unwrap_err();
        assert!(matches!(error, FinishSetupError::AlreadyDone), "{error}");

        let household = crate::household(&pool).await.unwrap().unwrap();
        assert_eq!(household.currency.code(), "CAD");
        let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
        let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!((users, sessions), (1, 1));
    }

    async fn sessions(pool: &SqlitePool) -> Vec<(String, String)> {
        sqlx::query_as("SELECT token_hash, expires_at FROM sessions ORDER BY token_hash")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn finds_a_password_login_by_username() {
        let (_dir, pool) = set_up().await;
        let sam = password_login(&pool, &Username::new("Sam").unwrap())
            .await
            .unwrap()
            .unwrap();
        assert!(sam.password_hash.starts_with("$argon2id$"));
        assert_eq!(
            password_login(&pool, &Username::new("alex").unwrap())
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn removed_logins_and_logins_without_a_password_cant_log_in() {
        let (_dir, pool) = set_up().await;
        sqlx::query(
            "INSERT INTO users (username, display_name, remote_user) VALUES ('alex', 'Alex', 'alex')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE users SET removed_at = '2026-10-28T10:00:00.000Z' WHERE username = 'sam'",
        )
        .execute(&pool)
        .await
        .unwrap();
        for name in ["sam", "alex"] {
            let login = password_login(&pool, &Username::new(name).unwrap())
                .await
                .unwrap();
            assert_eq!(login, None, "{name}");
        }
    }

    #[tokio::test]
    async fn logging_in_writes_a_session_and_forgets_the_failures() {
        let (_dir, pool) = set_up().await;
        let sam = password_login(&pool, &Username::new("sam").unwrap())
            .await
            .unwrap()
            .unwrap();
        record_failure(&pool, "sam", at("2026-10-28T09:10:00Z"))
            .await
            .unwrap();
        record_failure(&pool, "alex", at("2026-10-28T09:10:00Z"))
            .await
            .unwrap();

        log_in(
            &pool,
            sam.user_id,
            "def456",
            "sam",
            at("2026-10-29T12:00:00Z"),
        )
        .await
        .unwrap();

        assert_eq!(
            sessions(&pool).await,
            [
                ("abc123".to_owned(), "2026-11-27T09:00:00.000Z".to_owned()),
                ("def456".to_owned(), "2026-11-28T12:00:00.000Z".to_owned()),
            ]
        );
        assert_eq!(recent_failures(&pool, "sam").await.unwrap(), []);
        assert_eq!(recent_failures(&pool, "alex").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn using_a_session_names_its_login_and_slides_its_expiry() {
        let (_dir, pool) = set_up().await;
        let login = use_session(&pool, "abc123", at("2026-11-20T08:00:00Z"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (login.username.as_str(), login.display_name.as_str()),
            ("sam", "Sam")
        );
        let (last_seen, expires): (String, String) =
            sqlx::query_as("SELECT last_seen_at, expires_at FROM sessions")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(last_seen, "2026-11-20T08:00:00.000Z");
        assert_eq!(expires, "2026-12-20T08:00:00.000Z");

        // Past the first expiry, but within 30 days of the last use: still live.
        assert!(
            use_session(&pool, "abc123", at("2026-12-01T08:00:00Z"))
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn an_expired_session_is_refused_and_deleted() {
        let (_dir, pool) = set_up().await;
        assert_eq!(
            use_session(&pool, "abc123", at("2026-11-27T09:00:00Z"))
                .await
                .unwrap(),
            None
        );
        assert_eq!(sessions(&pool).await, []);
    }

    #[tokio::test]
    async fn an_unknown_session_or_a_removed_logins_is_refused() {
        let (_dir, pool) = set_up().await;
        let now = at("2026-10-28T10:00:00Z");
        assert_eq!(use_session(&pool, "nope", now).await.unwrap(), None);
        sqlx::query("UPDATE users SET removed_at = '2026-10-28T09:30:00.000Z'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(use_session(&pool, "abc123", now).await.unwrap(), None);
    }

    #[tokio::test]
    async fn ending_a_session_deletes_only_it() {
        let (_dir, pool) = set_up().await;
        let sam = password_login(&pool, &Username::new("sam").unwrap())
            .await
            .unwrap()
            .unwrap();
        log_in(
            &pool,
            sam.user_id,
            "def456",
            "sam",
            at("2026-10-28T10:00:00Z"),
        )
        .await
        .unwrap();
        end_session(&pool, "abc123").await.unwrap();
        let left: Vec<String> = sessions(&pool).await.into_iter().map(|(t, _)| t).collect();
        assert_eq!(left, ["def456"]);
    }

    #[tokio::test]
    async fn a_failed_step_rolls_the_whole_setup_back() {
        let (_dir, pool) = migrated().await;
        sqlx::query("INSERT INTO users (username, display_name) VALUES ('sam', 'Someone')")
            .execute(&pool)
            .await
            .unwrap();
        let f = fixture();
        let error = finish_setup(&pool, &setup(&f), at("2026-10-28T09:00:00Z"))
            .await
            .unwrap_err();
        assert!(matches!(error, FinishSetupError::Db(_)), "{error}");
        assert_eq!(crate::household(&pool).await.unwrap(), None);
    }
}
