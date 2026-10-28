use std::time::Instant;

use axum::extract::Request;
use axum::http::{Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::ApiError;

/// Mutating requests must be JSON, which with `SameSite=Lax` cookies blocks cross-site form posts.
pub async fn require_json(request: Request, next: Next) -> Response {
    let mutating = matches!(
        *request.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );
    if mutating && !is_json(&request) {
        return ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "json_required",
            "Send this request as JSON, with Content-Type: application/json.",
        )
        .into_response();
    }
    next.run(request).await
}

fn is_json(request: &Request) -> bool {
    request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
}

/// One line per request: method, path, status and time taken.
/// Never the query string or body, which can hold amounts, merchants and descriptions.
pub async fn log_request(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let started = Instant::now();
    let response = next.run(request).await;
    let status = response.status().as_u16();
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    tracing::info!(%method, path, status, ms = format_args!("{ms:.1}"), "request");
    response
}
