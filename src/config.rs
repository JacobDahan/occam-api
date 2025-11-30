use serde::Deserialize;

/// Application configuration loaded from environment variables
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    /// PostgreSQL database connection URL
    #[serde(default = "default_database_url")]
    pub database_url: String,

    /// Redis connection URL
    #[serde(default = "default_redis_url")]
    pub redis_url: String,

    /// Streaming Availability API key
    pub streaming_api_key: String,

    /// Streaming Availability API base URL
    #[serde(default = "default_streaming_api_url")]
    pub streaming_api_url: String,

    /// Server host address
    #[serde(default = "default_host")]
    pub host: String,

    /// Server port
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_database_url() -> String {
    "postgres://postgres:postgres@localhost:5432/occam".to_string()
}

fn default_redis_url() -> String {
    "redis://localhost:6379".to_string()
}

fn default_streaming_api_url() -> String {
    "https://streaming-availability.p.rapidapi.com".to_string()
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    3000
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        envy::from_env::<Config>().map_err(|e| anyhow::anyhow!("Failed to load config: {}", e))
    }
}
