use axum::{
    http::StatusCode,
    middleware,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::middleware::request_id;
use crate::services::title_search::TitleSearcher;

pub mod titles;
pub mod optimize;
pub mod recommendations;

pub struct AppState {
    pub title_searcher: Arc<dyn TitleSearcher>,
}

/// Creates the application router with all routes
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .nest("/api/v1", api_routes())
        .fallback(handler_404)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(request_id::make_span_with_request_id)
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(middleware::from_fn(request_id::request_id_middleware))
        .with_state(state.title_searcher)
}

/// API routes under /api/v1
fn api_routes() -> Router<Arc<dyn TitleSearcher>> {
    Router::new()
        .route("/titles/search", get(titles::search))
        .route("/optimize", post(optimize::optimize))
        .route("/recommendations", post(recommendations::recommend))
}

/// Health check endpoint
async fn health_check() -> (StatusCode, Json<Value>) {
    (StatusCode::OK, Json(json!({ "status": "healthy" })))
}

/// 404 handler for unknown routes
async fn handler_404() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "Route not found" })),
    )
}
