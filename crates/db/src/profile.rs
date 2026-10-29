//! A login's own settings, password and sessions (Settings › Profile).

use domain::login::DisplayName;
use domain::profile::{Locale, WeekStart};

use crate::{Error, SqlitePool};

/// What Settings › Profile shows and changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub username: String,
    pub display_name: String,
    /// BCP 47; `None` = the browser's.
    pub display_locale: Option<String>,
    /// 0 = Sunday; `None` = from the locale.
    pub week_start: Option<u8>,
}

/// The login's profile.
///
/// # Errors
/// If the database fails, or there's no such login.
pub async fn profile(pool: &SqlitePool, user_id: i64) -> Result<Profile, Error> {
    let row = sqlx::query!(
        r#"SELECT username, display_name, display_locale, week_start AS "week_start: i64"
           FROM users WHERE id = ?"#,
        user_id
    )
    .fetch_one(pool)
    .await?;
    Ok(Profile {
        username: row.username,
        display_name: row.display_name,
        display_locale: row.display_locale,
        // The schema's CHECK keeps it to 0–6.
        week_start: row.week_start.and_then(|day| u8::try_from(day).ok()),
    })
}

/// What the profile form saves.
pub struct ProfileChange<'a> {
    pub display_name: &'a DisplayName,
    pub display_locale: Option<&'a Locale>,
    pub week_start: Option<WeekStart>,
}

/// Saves the login's display name, locale and week start.
///
/// # Errors
/// If the database fails.
pub async fn update_profile(
    pool: &SqlitePool,
    user_id: i64,
    change: &ProfileChange<'_>,
) -> Result<(), Error> {
    let display_name = change.display_name.as_str();
    let locale = change.display_locale.map(Locale::as_str);
    let week_start = change.week_start.map(|day| i64::from(day.day()));
    sqlx::query!(
        "UPDATE users SET display_name = ?, display_locale = ?, week_start = ? WHERE id = ?",
        display_name,
        locale,
        week_start,
        user_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// The login's password hash, in PHC string form, if it has one.
///
/// # Errors
/// If the database fails.
pub async fn password_hash(pool: &SqlitePool, user_id: i64) -> Result<Option<String>, Error> {
    let hash = sqlx::query_scalar!("SELECT password_hash FROM users WHERE id = ?", user_id)
        .fetch_optional(pool)
        .await?;
    Ok(hash.flatten())
}

/// Sets the login's password hash.
///
/// # Errors
/// If the database fails.
pub async fn set_password(pool: &SqlitePool, user_id: i64, hash: &str) -> Result<(), Error> {
    sqlx::query!(
        "UPDATE users SET password_hash = ? WHERE id = ?",
        hash,
        user_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Deletes every session the login has: logging out everywhere.
///
/// # Errors
/// If the database fails.
pub async fn end_all_sessions(pool: &SqlitePool, user_id: i64) -> Result<(), Error> {
    sqlx::query!("DELETE FROM sessions WHERE user_id = ?", user_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::set_up;

    async fn sam_id(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT id FROM users WHERE username = 'sam'")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_new_login_follows_the_browser() {
        let (_dir, pool) = set_up().await;
        let sam = sam_id(&pool).await;
        assert_eq!(
            profile(&pool, sam).await.unwrap(),
            Profile {
                username: "sam".to_owned(),
                display_name: "Sam".to_owned(),
                display_locale: None,
                week_start: None,
            }
        );
    }

    #[tokio::test]
    async fn saves_and_clears_the_profile() {
        let (_dir, pool) = set_up().await;
        let sam = sam_id(&pool).await;
        let name = DisplayName::new("Sam Lee").unwrap();
        let locale: Locale = "de-DE".parse().unwrap();
        let change = ProfileChange {
            display_name: &name,
            display_locale: Some(&locale),
            week_start: Some(WeekStart::new(1).unwrap()),
        };
        update_profile(&pool, sam, &change).await.unwrap();
        let saved = profile(&pool, sam).await.unwrap();
        assert_eq!(saved.display_name, "Sam Lee");
        assert_eq!(saved.display_locale.as_deref(), Some("de-DE"));
        assert_eq!(saved.week_start, Some(1));

        let cleared = ProfileChange {
            display_locale: None,
            week_start: None,
            ..change
        };
        update_profile(&pool, sam, &cleared).await.unwrap();
        let saved = profile(&pool, sam).await.unwrap();
        assert_eq!((saved.display_locale, saved.week_start), (None, None));
    }

    #[tokio::test]
    async fn sets_the_password() {
        let (_dir, pool) = set_up().await;
        let sam = sam_id(&pool).await;
        assert!(
            password_hash(&pool, sam)
                .await
                .unwrap()
                .unwrap()
                .starts_with("$argon2id$")
        );
        set_password(&pool, sam, "$argon2id$new").await.unwrap();
        assert_eq!(
            password_hash(&pool, sam).await.unwrap().as_deref(),
            Some("$argon2id$new")
        );
        assert_eq!(password_hash(&pool, sam + 1).await.unwrap(), None);
    }

    #[tokio::test]
    async fn ends_every_session_of_the_login_and_only_its() {
        let (_dir, pool) = set_up().await;
        let sam = sam_id(&pool).await;
        sqlx::query("INSERT INTO users (id, username, display_name) VALUES (?, 'alex', 'Alex')")
            .bind(sam + 1)
            .execute(&pool)
            .await
            .unwrap();
        let now: jiff::Timestamp = "2026-10-28T10:00:00Z".parse().unwrap();
        crate::log_in(&pool, sam, "phone", "sam", now)
            .await
            .unwrap();
        crate::log_in(&pool, sam + 1, "alex", "alex", now)
            .await
            .unwrap();

        end_all_sessions(&pool, sam).await.unwrap();
        let left: Vec<String> = sqlx::query_scalar("SELECT token_hash FROM sessions")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(left, ["alex"]);
    }
}
