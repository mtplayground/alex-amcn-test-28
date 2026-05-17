//! HTTP application bootstrap for the backend workspace member.

pub mod config;
pub mod db;
pub mod domain;
pub mod executor;
pub mod graph;
pub mod lexer;
pub mod parser;

use axum::{routing::get, Json, Router};
use serde::Serialize;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

/// Returns the backend workspace member name.
pub fn crate_name() -> &'static str {
    "zeroclaw-server"
}

/// Builds the application router.
pub fn app() -> Router {
    Router::new()
        .nest("/api", api_router())
        .fallback_service(spa_service())
        .layer(TraceLayer::new_for_http())
}

fn api_router() -> Router {
    Router::new().route("/health", get(health_check))
}

fn spa_service() -> ServeDir<ServeFile> {
    ServeDir::new("frontend/dist").fallback(ServeFile::new("frontend/dist/index.html"))
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

#[cfg(test)]
mod tests {
    use super::{app, crate_name};
    use axum::body::Body;
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    #[test]
    fn exposes_crate_name() {
        assert_eq!(crate_name(), "zeroclaw-server");
    }

    #[tokio::test]
    async fn health_check_returns_ok_payload() {
        let request_result = Request::builder().uri("/api/health").body(Body::empty());
        assert!(request_result.is_ok());

        let response_result = app()
            .oneshot(match request_result {
                Ok(request) => request,
                Err(error) => panic!("request should build: {error}"),
            })
            .await;
        assert!(response_result.is_ok());

        let response = match response_result {
            Ok(response) => response,
            Err(error) => panic!("router should respond: {error}"),
        };

        assert_eq!(response.status(), StatusCode::OK);
    }
}
