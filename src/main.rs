mod config;
mod db;
mod error;
mod models;
mod routes;
mod services;

use config::Config;
use routes::AppState;
use services::title_search::TitleSearchService;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "occam_api=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration
    let config = Config::from_env()?;

    // Initialize database connection pool
    let db_pool = db::create_pool(&config.database_url).await?;
    tracing::info!("Connected to PostgreSQL");

    // Run migrations
    sqlx::migrate!("./migrations").run(&db_pool).await?;
    tracing::info!("Migrations complete");

    // Initialize Redis client
    let redis_client = db::create_redis_client(&config.redis_url)?;
    tracing::info!("Connected to Redis");

    // Initialize services
    let title_searcher = Arc::new(TitleSearchService::new(
        redis_client,
        config.streaming_api_key.clone(),
        config.streaming_api_url.clone(),
    ));

    // Create application state
    let app_state = AppState { title_searcher };

    // Create application router
    let app = routes::create_router(app_state);

    // Create server address
    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!("Server listening on {}", addr);

    // Start server
    axum::serve(listener, app).await?;

    Ok(())
}
