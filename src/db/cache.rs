use redis::Client;

/// Creates a Redis client for caching
///
/// Establishes a connection to Redis for fast data caching.
/// Uses connection pooling via the connection-manager feature.
pub fn create_redis_client(redis_url: &str) -> anyhow::Result<Client> {
    let client = Client::open(redis_url)?;
    Ok(client)
}
