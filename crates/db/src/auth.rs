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

    let mut tx = pool.begin().await?;
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
    use crate::tests::migrated;

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
