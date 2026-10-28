//! `budget`: the whole app in one binary.

mod config;
mod web;

use std::process::ExitCode;

use config::Config;
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
    #[error("can't create the data dir {0}: {1}")]
    DataDir(String, std::io::Error),
    #[error(transparent)]
    Db(#[from] db::Error),
    #[error("can't listen on {0}: {1}")]
    Listen(std::net::SocketAddr, std::io::Error),
    #[error("server error: {0}")]
    Serve(std::io::Error),
}

async fn run(config: Config) -> Result<(), StartError> {
    std::fs::create_dir_all(&config.data_dir)
        .map_err(|error| StartError::DataDir(config.data_dir.display().to_string(), error))?;
    let pool = db::connect(&config.data_dir.join(db::FILE_NAME)).await?;
    db::migrate(&pool).await?;

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
    let app = web::app(api::router(api::AppState { pool: pool.clone() }), dist);
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
