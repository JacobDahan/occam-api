mod config;
mod db;
mod error;
mod middleware;
mod models;
mod routes;
mod services;

use config::{Config, StreamingProviderType};
use routes::AppState;
use services::providers::{
    streaming_availability::StreamingAvailabilityProvider, watchmode::WatchmodeProvider,
    StreamingProvider,
};
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

    // Initialize Redis client and cache
    let redis_client = db::create_redis_client(&config.redis_url)?;
    let cache = db::Cache::new(redis_client.clone());
    tracing::info!("Connected to Redis");

    // Initialize streaming provider based on configuration
    let streaming_provider: Arc<dyn StreamingProvider> = match config.streaming_provider {
        StreamingProviderType::StreamingAvailability => {
            tracing::info!("Using Streaming Availability API provider");
            Arc::new(StreamingAvailabilityProvider::new(
                cache,
                config.streaming_api_key.clone(),
                config.streaming_api_url.clone(),
            ))
        }
        StreamingProviderType::Watchmode => {
            tracing::info!("Using Watchmode API provider");
            Arc::new(
                WatchmodeProvider::new(
                    cache,
                    db_pool.clone(),
                    config.streaming_api_key.clone(),
                    config.streaming_api_url.clone(),
                )
                .await?,
            )
        }
    };

    // Create application state
    let app_state = AppState {
        db_pool: Arc::new(db_pool),
        streaming_provider,
    };

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
