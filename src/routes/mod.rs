use axum::{
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};

pub mod titles;
pub mod optimize;
pub mod recommendations;

/// Creates the application router with all routes
pub fn create_router() -> Router {
    Router::new()
        .route("/health", get(health_check))
        .nest("/api/v1", api_routes())
}

/// API routes under /api/v1
fn api_routes() -> Router {
    Router::new()
        .route("/titles/search", get(titles::search))
        .route("/optimize", post(optimize::optimize))
        .route("/recommendations", post(recommendations::recommend))
}

/// Health check endpoint
async fn health_check() -> (StatusCode, Json<Value>) {
    (StatusCode::OK, Json(json!({ "status": "healthy" })))
}
