use axum::{
    routing::{get, post},
    Router,
};

use super::handlers;
use super::AppState;

/// Creates the main API router with all routes
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(handlers::health_check))
        // Streaming services
        .route("/services", get(handlers::get_services))
        .route("/services", post(handlers::create_service))
        // Titles
        .route("/titles", get(handlers::get_titles))
        .route("/titles", post(handlers::create_title))
        // User preferences
        .route("/preferences", get(handlers::get_preferences))
        .route("/preferences/titles", post(handlers::add_title_preference))
        .route("/preferences/subscriptions", post(handlers::add_subscription))
        // Optimization
        .route("/optimize", get(handlers::optimize))
        .with_state(state)
}
