mod api;
mod models;
mod services;

use api::{create_router, AppState};

#[tokio::main]
async fn main() {
    // Initialize application state
    let state = AppState::new();

    // Create the router with all routes
    let app = create_router(state);

    // Start the server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Server running on http://localhost:3000");
    axum::serve(listener, app).await.unwrap();
}

