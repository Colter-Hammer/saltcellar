//! `budget reset-password <login>`: recovery without email (docs/spec/foundations.md#auth). Run in
//! the app's container, it prints a one-time link that sets a new password. The link is the one
//! secret it prints, and only to the person running it: never to the log.

use std::path::PathBuf;

use jiff::Timestamp;

use crate::config::Config;

#[derive(Debug, thiserror::Error)]
pub enum ResetError {
    #[error("there's no database at {0}; is BUDGET_DATA_DIR the app's?")]
    NoDatabase(PathBuf),
    #[error(transparent)]
    Db(#[from] db::Error),
    #[error(transparent)]
    Issue(#[from] api::IssueResetError),
}

/// Issues a reset link for the login named `username`, at `now`, and says how to use it: the
/// whole link when `BUDGET_BASE_URL` is set, else its path, to go after the app's address
/// *(agent)*.
///
/// # Errors
/// If there's no database, no such login, or the database fails.
pub async fn run(config: &Config, username: &str, now: Timestamp) -> Result<String, ResetError> {
    let file = config.data_dir.join(db::FILE_NAME);
    // Connecting would make an empty database where there's none.
    if !file.is_file() {
        return Err(ResetError::NoDatabase(file));
    }
    let pool = db::connect(&file).await?;
    let issued = api::issue_password_reset(&pool, username, now).await;
    pool.close().await;
    let path = issued?.path();
    let about = format!(
        "A password reset link for {}. It works once, within 24 hours; a newer one replaces it.",
        username.trim()
    );
    Ok(match &config.base_url {
        Some(base) => format!("{about}\n\n  {base}{path}\n"),
        None => format!(
            "{about}\nBUDGET_BASE_URL isn't set, so this is only the link's path: put the app's address in front of it, like http://localhost:8080/reset-password#…\n\n  {path}\n"
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Install {
        config: Config,
        _data: tempfile::TempDir,
    }

    /// A finished setup with the login `sam`.
    async fn install(base_url: Option<&str>) -> Install {
        let data = tempfile::tempdir().unwrap();
        let pool = db::connect(&data.path().join(db::FILE_NAME)).await.unwrap();
        db::migrate(&pool).await.unwrap();
        finish_setup(&pool).await;
        pool.close().await;
        let config = Config::from_env(|name| match name {
            "BUDGET_DATA_DIR" => Some(data.path().display().to_string()),
            "BUDGET_BASE_URL" => base_url.map(str::to_owned),
            _ => None,
        })
        .unwrap();
        Install {
            config,
            _data: data,
        }
    }

    /// Setup, through the API the way the wizard does it.
    async fn finish_setup(pool: &db::SqlitePool) {
        use axum::body::Body;
        use axum::http::{Request, StatusCode, header};
        use tower::ServiceExt;

        let state = api::AppState::load(pool.clone()).await.unwrap();
        let code = state.setup_code().unwrap().to_string();
        let body = format!(
            r#"{{"code":"{code}","currency":"EUR","timezone":"UTC","display_name":"Sam","username":"sam","password":"correct horse battery","password_again":"correct horse battery"}}"#
        );
        let request = Request::post("/api/v1/setup")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap();
        let response = api::router(state).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    fn now() -> Timestamp {
        "2026-10-28T09:00:00Z".parse().unwrap()
    }

    fn link_in(out: &str) -> &str {
        out.lines().last().unwrap().trim()
    }

    #[tokio::test]
    async fn prints_the_whole_link_with_a_base_url() {
        let install = install(Some("https://budget.example.test/")).await;
        let out = run(&install.config, "Sam", now()).await.unwrap();
        assert!(
            out.starts_with("A password reset link for Sam. It works once, within 24 hours"),
            "{out}"
        );
        let link = link_in(&out);
        let token = link
            .strip_prefix("https://budget.example.test/reset-password#")
            .unwrap();
        assert_eq!(token.len(), 64);
        assert_eq!(out.matches(token).count(), 1);
    }

    #[tokio::test]
    async fn prints_the_path_and_says_what_to_put_before_it_without_one() {
        let install = install(None).await;
        let out = run(&install.config, "sam", now()).await.unwrap();
        assert!(out.contains("BUDGET_BASE_URL isn't set"), "{out}");
        assert!(
            out.contains("put the app's address in front of it"),
            "{out}"
        );
        assert!(link_in(&out).starts_with("/reset-password#"), "{out}");
    }

    #[tokio::test]
    async fn refuses_an_unknown_login() {
        let install = install(None).await;
        let error = run(&install.config, "alex", now()).await.unwrap_err();
        assert_eq!(error.to_string(), "there's no login named \"alex\"");
    }

    #[tokio::test]
    async fn doesnt_make_a_database_where_there_is_none() {
        let data = tempfile::tempdir().unwrap();
        let config = Config::from_env(|name| {
            (name == "BUDGET_DATA_DIR").then(|| data.path().display().to_string())
        })
        .unwrap();
        let error = run(&config, "sam", now()).await.unwrap_err();
        assert!(matches!(error, ResetError::NoDatabase(_)), "{error}");
        assert!(!data.path().join(db::FILE_NAME).exists());
    }
}
