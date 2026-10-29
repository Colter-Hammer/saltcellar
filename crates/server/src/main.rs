//! `budget`: the whole app in one binary. `budget healthcheck` asks a running one for its health.

mod config;
mod healthcheck;
mod web;

use std::process::ExitCode;

use config::{Auth, Config};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    let config = match Config::from_env(|name| std::env::var(name).ok()) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("budget: can't start: {error}");
            return ExitCode::from(2);
        }
    };

    match std::env::args().nth(1).as_deref() {
        None => {}
        Some("healthcheck") => {
            return match healthcheck::check(config.listen) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("budget: unhealthy: {error}");
                    ExitCode::FAILURE
                }
            };
        }
        Some(other) => {
            eprintln!("budget: unknown command {other:?}; expected none or healthcheck");
            return ExitCode::from(2);
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    match run(config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("can't start: {error}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum StartError {
    /// Trusted-header mode is milestone 6's; config reads it already so the set of variables is
    /// fixed *(agent)*.
    #[error("BUDGET_AUTH_MODE is header, but header mode arrives in a later version")]
    HeaderMode,
    #[error("can't create the data dir {0}: {1}")]
    DataDir(String, std::io::Error),
    #[error(transparent)]
    Db(#[from] db::Error),
    #[error(transparent)]
    State(#[from] api::LoadError),
    #[error("can't listen on {0}: {1}")]
    Listen(std::net::SocketAddr, std::io::Error),
    #[error("server error: {0}")]
    Serve(std::io::Error),
}

/// What this version can't do yet, refused before anything starts.
fn check_supported(config: &Config) -> Result<(), StartError> {
    match config.auth {
        Auth::Builtin => Ok(()),
        Auth::Header { .. } => Err(StartError::HeaderMode),
    }
}

async fn run(config: Config) -> Result<(), StartError> {
    check_supported(&config)?;
    std::fs::create_dir_all(&config.data_dir)
        .map_err(|error| StartError::DataDir(config.data_dir.display().to_string(), error))?;
    let pool = db::connect(&config.data_dir.join(db::FILE_NAME)).await?;
    db::migrate(&pool).await?;
    let state = api::AppState::load(pool.clone()).await?;

    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .map_err(|error| StartError::Listen(config.listen, error))?;
    tracing::info!(listen = %config.listen, data_dir = %config.data_dir.display(), "listening");

    let dist = std::path::Path::new(web::DIST);
    if !dist.join("index.html").is_file() {
        tracing::warn!(
            dist = web::DIST,
            "no built frontend; only the API is served (build it with `npm --prefix web run build`)"
        );
    }
    if let Some(code) = state.setup_code() {
        // The one secret logged on purpose (docs/spec/foundations.md#logging).
        tracing::info!(
            "Setup code: {code}. Open the app and enter it to set it up; a new code is printed each time the app starts until then."
        );
    }
    let app = web::app(api::router(state), dist);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await
        .map_err(StartError::Serve)?;
    pool.close().await;
    tracing::info!("stopped");
    Ok(())
}

/// Ctrl-C, or SIGTERM from `docker stop`.
async fn shutdown() {
    let interrupt = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut terminate =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(signal) => signal,
                Err(error) => {
                    tracing::warn!(%error, "can't watch for SIGTERM; only Ctrl-C stops the server");
                    let _ = interrupt.await;
                    return;
                }
            };
        tokio::select! {
            _ = interrupt => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = interrupt.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_mode_is_refused_until_it_arrives() {
        let builtin = Config::from_env(|_| None).unwrap();
        assert!(check_supported(&builtin).is_ok());

        let header = Config::from_env(|name| match name {
            "BUDGET_AUTH_MODE" => Some("header".to_owned()),
            "BUDGET_TRUSTED_PROXY" => Some("10.0.0.5".to_owned()),
            _ => None,
        })
        .unwrap();
        let error = check_supported(&header).unwrap_err();
        assert_eq!(
            error.to_string(),
            "BUDGET_AUTH_MODE is header, but header mode arrives in a later version"
        );
    }
}
