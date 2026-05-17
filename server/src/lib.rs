//! HTTP application bootstrap for the backend workspace member.

pub mod config;
pub mod db;
pub mod domain;
pub mod executor;
pub mod graph;
pub mod lexer;
pub mod parser;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tokio::sync::Mutex;

use crate::{
    db::{NodeRepo, RelRepo},
    domain::GraphData,
    executor::execute_query,
    graph::GraphIndex,
    parser::parse,
};

/// Returns the backend workspace member name.
pub fn crate_name() -> &'static str {
    "zeroclaw-server"
}

/// Builds the application router.
pub fn app() -> Router {
    Router::new()
        .nest("/api", Router::new().route("/health", get(health_check)))
        .fallback_service(spa_service())
        .layer(TraceLayer::new_for_http())
}

/// Builds the application router with database-backed API routes.
pub fn app_with_state(pool: PgPool, graph_index: GraphIndex) -> Router {
    Router::new()
        .nest("/api", api_router())
        .with_state(AppState {
            pool,
            graph_index: Arc::new(Mutex::new(graph_index)),
        })
        .fallback_service(spa_service())
        .layer(TraceLayer::new_for_http())
}

fn api_router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health_check))
        .route("/graph", get(graph_handler))
        .route("/query", post(query_handler))
}

fn spa_service() -> ServeDir<ServeFile> {
    ServeDir::new("frontend/dist").fallback(ServeFile::new("frontend/dist/index.html"))
}

#[derive(Debug, Clone)]
struct AppState {
    pool: PgPool,
    graph_index: Arc<Mutex<GraphIndex>>,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

#[derive(Debug, Deserialize)]
struct GraphQuery {
    limit: Option<usize>,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    body: Json<ApiErrorBody>,
}

#[derive(Debug, Serialize)]
struct ApiErrorBody {
    error: ApiErrorPayload,
}

#[derive(Debug, Serialize)]
struct ApiErrorPayload {
    message: String,
    line: Option<usize>,
    column: Option<usize>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, self.body).into_response()
    }
}

async fn graph_handler(
    State(state): State<AppState>,
    query: Result<Query<GraphQuery>, axum::extract::rejection::QueryRejection>,
) -> Result<Json<GraphData>, ApiError> {
    let Query(query) = query.map_err(|error| ApiError {
        status: StatusCode::BAD_REQUEST,
        body: Json(ApiErrorBody {
            error: ApiErrorPayload {
                message: error.body_text(),
                line: None,
                column: None,
            },
        }),
    })?;
    let node_repo = NodeRepo::new(state.pool.clone());
    let rel_repo = RelRepo::new(state.pool);

    let (nodes, relationships) = match query.limit {
        Some(limit) => (
            node_repo.list_limit(limit).await,
            rel_repo.list_limit(limit).await,
        ),
        None => (node_repo.list().await, rel_repo.list().await),
    };

    Ok(Json(GraphData {
        nodes: nodes.map_err(internal_error)?,
        relationships: relationships.map_err(internal_error)?,
    }))
}

fn internal_error(error: sqlx::Error) -> ApiError {
    ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        body: Json(ApiErrorBody {
            error: ApiErrorPayload {
                message: error.to_string(),
                line: None,
                column: None,
            },
        }),
    }
}

#[derive(Debug, Deserialize)]
struct QueryRequest {
    query: String,
}

async fn query_handler(
    State(state): State<AppState>,
    Json(request): Json<QueryRequest>,
) -> Result<Json<crate::domain::QueryResult>, ApiError> {
    let parsed = parse(&request.query).map_err(|error| ApiError {
        status: StatusCode::BAD_REQUEST,
        body: Json(ApiErrorBody {
            error: ApiErrorPayload {
                message: error.message,
                line: Some(error.line),
                column: Some(error.column),
            },
        }),
    })?;
    let mut graph_index = state.graph_index.lock().await;

    execute_query(&state.pool, &mut graph_index, &parsed)
        .await
        .map(Json)
        .map_err(|error| ApiError {
            status: StatusCode::BAD_REQUEST,
            body: Json(ApiErrorBody {
                error: ApiErrorPayload {
                    message: error.message,
                    line: None,
                    column: None,
                },
            }),
        })
}

#[cfg(test)]
mod tests {
    use super::{app, app_with_state, crate_name};
    use crate::{
        db::{create_pool, NodeRepo, RelRepo},
        domain::Properties,
        graph::GraphIndex,
    };
    use axum::body::{to_bytes, Body};
    use http::{Request, StatusCode};
    use serde_json::{json, Value};
    use std::sync::OnceLock;
    use tokio::sync::Mutex;
    use tower::ServiceExt;

    static DB_TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

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

    #[tokio::test]
    async fn graph_endpoint_returns_full_dataset_and_applies_limit() {
        let _guard = DB_TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        ensure_schema(&pool).await;
        reset_tables(&pool).await;

        let node_repo = NodeRepo::new(pool.clone());
        let rel_repo = RelRepo::new(pool.clone());

        let service = node_repo
            .insert(vec!["Service".to_string()], Properties::new())
            .await
            .expect("service insert should succeed");
        let database = node_repo
            .insert(vec!["Database".to_string()], Properties::new())
            .await
            .expect("database insert should succeed");
        let queue = node_repo
            .insert(vec!["Queue".to_string()], Properties::new())
            .await
            .expect("queue insert should succeed");

        let depends_on = rel_repo
            .insert(
                "DEPENDS_ON".to_string(),
                service.id,
                database.id,
                Properties::new(),
            )
            .await
            .expect("depends_on insert should succeed");
        rel_repo
            .insert(
                "PUBLISHES_TO".to_string(),
                service.id,
                queue.id,
                Properties::new(),
            )
            .await
            .expect("publishes_to insert should succeed");

        let graph_index = GraphIndex::load_from_db(&pool)
            .await
            .expect("graph index should load");
        let response = app_with_state(pool.clone(), graph_index)
            .oneshot(
                Request::builder()
                    .uri("/api/graph")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("router should respond");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should read");
        let payload: Value = serde_json::from_slice(&body).expect("payload should deserialize");
        assert_eq!(payload["nodes"].as_array().expect("nodes should be array").len(), 3);
        assert_eq!(
            payload["relationships"]
                .as_array()
                .expect("relationships should be array")
                .len(),
            2
        );
        assert_eq!(payload["relationships"][0]["id"], json!(depends_on.id));

        let graph_index = GraphIndex::load_from_db(&pool)
            .await
            .expect("graph index should load");
        let limited_response = app_with_state(pool, graph_index)
            .oneshot(
                Request::builder()
                    .uri("/api/graph?limit=1")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("router should respond");

        assert_eq!(limited_response.status(), StatusCode::OK);
        let body = to_bytes(limited_response.into_body(), usize::MAX)
            .await
            .expect("body should read");
        let payload: Value = serde_json::from_slice(&body).expect("payload should deserialize");
        assert_eq!(payload["nodes"].as_array().expect("nodes should be array").len(), 1);
        assert_eq!(
            payload["relationships"]
                .as_array()
                .expect("relationships should be array")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn graph_endpoint_rejects_invalid_limit_query() {
        let Some(pool) = test_pool().await else {
            return;
        };

        let response = app_with_state(pool, GraphIndex::new())
            .oneshot(
                Request::builder()
                    .uri("/api/graph?limit=abc")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("router should respond");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn query_endpoint_executes_match_and_returns_query_result() {
        let _guard = DB_TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().await;
        let Some(pool) = test_pool().await else {
            return;
        };
        ensure_schema(&pool).await;
        reset_tables(&pool).await;

        let node_repo = NodeRepo::new(pool.clone());
        let rel_repo = RelRepo::new(pool.clone());

        let service = node_repo
            .insert(vec!["Service".to_string()], Properties::new())
            .await
            .expect("service insert should succeed");
        let database = node_repo
            .insert(vec!["Database".to_string()], Properties::new())
            .await
            .expect("database insert should succeed");
        rel_repo
            .insert(
                "DEPENDS_ON".to_string(),
                service.id,
                database.id,
                Properties::new(),
            )
            .await
            .expect("relationship insert should succeed");

        let graph_index = GraphIndex::load_from_db(&pool)
            .await
            .expect("graph index should load");
        let response = app_with_state(pool, graph_index)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/query")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"query":"MATCH (service:Service)-[rel:DEPENDS_ON]->(database:Database) RETURN service, rel, database"}"#,
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("router should respond");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should read");
        let payload: Value = serde_json::from_slice(&body).expect("payload should deserialize");
        assert_eq!(payload["columns"], json!(["service", "rel", "database"]));
        assert_eq!(payload["rows"].as_array().expect("rows should be array").len(), 1);
        assert_eq!(payload["nodes"].as_array().expect("nodes should be array").len(), 2);
        assert_eq!(
            payload["relationships"]
                .as_array()
                .expect("relationships should be array")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn query_endpoint_returns_structured_parse_errors() {
        let Some(pool) = test_pool().await else {
            return;
        };

        let response = app_with_state(pool, GraphIndex::new())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/query")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"MATCH (n {name: }) RETURN n"}"#))
                    .expect("request should build"),
            )
            .await
            .expect("router should respond");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should read");
        let payload: Value = serde_json::from_slice(&body).expect("payload should deserialize");
        assert_eq!(payload["error"]["message"], json!("expected a literal value"));
        assert_eq!(payload["error"]["line"], json!(1));
        assert_eq!(payload["error"]["column"], json!(18));
    }

    #[tokio::test]
    async fn query_endpoint_returns_structured_execution_errors() {
        let Some(pool) = test_pool().await else {
            return;
        };

        let response = app_with_state(pool, GraphIndex::new())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/query")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"MATCH (n) RETURN missing"}"#))
                    .expect("request should build"),
            )
            .await
            .expect("router should respond");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should read");
        let payload: Value = serde_json::from_slice(&body).expect("payload should deserialize");
        assert_eq!(payload["error"]["message"], json!("unbound identifier 'missing'"));
        assert!(payload["error"]["line"].is_null());
        assert!(payload["error"]["column"].is_null());
    }

    async fn test_pool() -> Option<sqlx::PgPool> {
        let database_url = std::env::var("ZEROCLAW_TEST_DATABASE_URL")
            .unwrap_or_else(|_| include_str!("../../.database_url").trim().to_string());

        match create_pool(&database_url).await {
            Ok(pool) => Some(pool),
            Err(error) => {
                eprintln!(
                    "skipping database-backed HTTP test because the configured database is unavailable: {error}"
                );
                None
            }
        }
    }

    async fn ensure_schema(pool: &sqlx::PgPool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS nodes (
                id BIGSERIAL PRIMARY KEY,
                labels TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
                properties JSONB NOT NULL DEFAULT '{}'::JSONB
            );

            CREATE TABLE IF NOT EXISTS relationships (
                id BIGSERIAL PRIMARY KEY,
                type TEXT NOT NULL,
                start_id BIGINT NOT NULL,
                end_id BIGINT NOT NULL,
                properties JSONB NOT NULL DEFAULT '{}'::JSONB
            );

            CREATE INDEX IF NOT EXISTS nodes_labels_gin_idx ON nodes USING GIN (labels);
            CREATE INDEX IF NOT EXISTS relationships_type_idx ON relationships (type);
            CREATE INDEX IF NOT EXISTS relationships_start_id_idx ON relationships (start_id);
            CREATE INDEX IF NOT EXISTS relationships_end_id_idx ON relationships (end_id);
            "#,
        )
        .execute(pool)
        .await
        .expect("schema setup should succeed");
    }

    async fn reset_tables(pool: &sqlx::PgPool) {
        sqlx::query(
            r#"
            TRUNCATE TABLE relationships, nodes RESTART IDENTITY
            "#,
        )
        .execute(pool)
        .await
        .expect("table reset should succeed");
    }
}
