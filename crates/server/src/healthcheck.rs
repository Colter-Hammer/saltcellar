//! `budget healthcheck`: the Docker `HEALTHCHECK`. The runtime image has no shell or curl, so the
//! binary asks its own `/api/v1/health` over plain HTTP and exits 0 only on a 200.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(3);

/// Why the server isn't healthy.
#[derive(Debug, thiserror::Error)]
pub enum Unhealthy {
    #[error("can't reach {0}: {1}")]
    Unreachable(SocketAddr, std::io::Error),
    #[error("health answered {0:?}")]
    Status(String),
}

/// Asks the server listening on `listen` for its health.
///
/// # Errors
/// The server can't be reached, or answers anything but 200.
pub fn check(listen: SocketAddr) -> Result<(), Unhealthy> {
    let target = reachable(listen);
    let unreachable = |error| Unhealthy::Unreachable(target, error);

    let mut stream = TcpStream::connect_timeout(&target, TIMEOUT).map_err(unreachable)?;
    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(unreachable)?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(unreachable)?;
    stream
        .write_all(
            format!("GET /api/v1/health HTTP/1.1\r\nHost: {target}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .map_err(unreachable)?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).map_err(unreachable)?;
    let response = String::from_utf8_lossy(&response);
    let status_line = response.lines().next().unwrap_or_default();
    if status_line.split(' ').nth(1) == Some("200") {
        Ok(())
    } else {
        Err(Unhealthy::Status(status_line.to_owned()))
    }
}

/// A wildcard listen address (`0.0.0.0`, `::`) is reached through loopback.
fn reachable(listen: SocketAddr) -> SocketAddr {
    let ip = match listen.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    SocketAddr::new(ip, listen.port())
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::http::StatusCode;
    use axum::routing::get;

    use super::*;

    /// Serves `router` on a free loopback port, in the background.
    async fn serve(router: Router) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        addr
    }

    async fn check_in_background(addr: SocketAddr) -> Result<(), Unhealthy> {
        tokio::task::spawn_blocking(move || check(addr))
            .await
            .unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_healthy_server_passes() {
        let data = tempfile::tempdir().unwrap();
        let pool = db::connect(&data.path().join(db::FILE_NAME)).await.unwrap();
        db::migrate(&pool).await.unwrap();
        let addr = serve(api::router(api::AppState::load(pool).await.unwrap())).await;

        check_in_background(addr).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_wildcard_listen_address_is_checked_on_loopback() {
        let addr = serve(Router::new().route("/api/v1/health", get(|| async { "ok" }))).await;
        let wildcard = SocketAddr::from(([0, 0, 0, 0], addr.port()));

        check_in_background(wildcard).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_failing_health_fails() {
        let router = Router::new().route(
            "/api/v1/health",
            get(|| async { StatusCode::SERVICE_UNAVAILABLE }),
        );
        let addr = serve(router).await;

        let error = check_in_background(addr).await.unwrap_err();
        assert!(
            matches!(&error, Unhealthy::Status(line) if line.contains("503")),
            "{error}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn no_server_fails() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let error = check_in_background(addr).await.unwrap_err();
        assert!(matches!(error, Unhealthy::Unreachable(..)), "{error}");
    }
}
