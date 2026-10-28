//! The built frontend (`web/dist`), served beside the API.

use std::path::Path;

use axum::Router;
use tower_http::services::{ServeDir, ServeFile};

/// Where the built frontend is, relative to the working directory.
pub const DIST: &str = "web/dist";

/// The API, plus the frontend for every other path. A path that isn't a built file gets
/// `index.html`, so a reload anywhere in the app lands back in it; `/api/…` is always the API's.
pub fn app(api: Router, dist: &Path) -> Router {
    let index = ServeFile::new(dist.join("index.html"));
    api.fallback_service(ServeDir::new(dist).fallback(index))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;

    const INDEX: &str = "<!doctype html><div id=\"app\"></div>";

    struct Site {
        router: Router,
        _data: tempfile::TempDir,
        _dist: tempfile::TempDir,
    }

    async fn site() -> Site {
        let dist = tempfile::tempdir().unwrap();
        std::fs::write(dist.path().join("index.html"), INDEX).unwrap();
        std::fs::create_dir(dist.path().join("assets")).unwrap();
        std::fs::write(dist.path().join("assets/app.js"), "console.log(1)").unwrap();

        let data = tempfile::tempdir().unwrap();
        let pool = db::connect(&data.path().join(db::FILE_NAME)).await.unwrap();
        db::migrate(&pool).await.unwrap();
        let router = app(api::router(api::AppState { pool }), dist.path());
        Site {
            router,
            _data: data,
            _dist: dist,
        }
    }

    async fn get(site: &Site, uri: &str) -> (StatusCode, Option<String>, String) {
        let request = Request::get(uri).body(Body::empty()).unwrap();
        let response = site.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|value| value.to_str().unwrap().to_owned());
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            content_type,
            String::from_utf8(bytes.to_vec()).unwrap(),
        )
    }

    #[tokio::test]
    async fn serves_the_app_at_the_root() {
        let (status, content_type, body) = get(&site().await, "/").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type.as_deref(), Some("text/html"));
        assert_eq!(body, INDEX);
    }

    #[tokio::test]
    async fn serves_built_assets() {
        let (status, content_type, body) = get(&site().await, "/assets/app.js").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type.as_deref(), Some("text/javascript"));
        assert_eq!(body, "console.log(1)");
    }

    #[tokio::test]
    async fn unknown_paths_reload_into_the_app() {
        let site = site().await;
        for uri in ["/transactions", "/transactions/whatever", "/more?tab=1"] {
            let (status, _, body) = get(&site, uri).await;
            assert_eq!(status, StatusCode::OK, "{uri}");
            assert_eq!(body, INDEX, "{uri}");
        }
    }

    #[tokio::test]
    async fn unknown_api_paths_stay_json() {
        let (status, content_type, body) = get(&site().await, "/api/nope").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(content_type.as_deref(), Some("application/json"));
        assert!(body.contains("\"not_found\""), "{body}");
    }

    #[tokio::test]
    async fn the_api_still_answers() {
        let (status, _, body) = get(&site().await, "/api/v1/health").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, r#"{"status":"ok"}"#);
    }
}
