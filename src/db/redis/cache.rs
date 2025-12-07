use redis::AsyncCommands;
use redis::Client;
use std::fmt::Display;
use tokio::sync::mpsc;

use crate::error::AppError;
use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CacheKey {
    TitleSearch(String),
    Availability(String),
    ImdbToWatchmode(String),
}

impl Display for CacheKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheKey::TitleSearch(query) => write!(f, "search:{}", query.to_lowercase()),
            CacheKey::Availability(id) => write!(f, "avail:{}", id),
            CacheKey::ImdbToWatchmode(imdb_id) => write!(f, "imdb2wm:{}", imdb_id),
        }
    }
}

/// Creates a Redis client for caching
///
/// Establishes a connection to Redis for fast data caching.
/// Uses connection pooling via the connection-manager feature.
pub fn create_redis_client(redis_url: &str) -> anyhow::Result<Client> {
    let client = Client::open(redis_url)?;
    Ok(client)
}

/// Message for asynchronous cache writes
struct CacheWriteMessage {
    key: String,
    value: String,
    ttl: u64,
}

/// Cache handler for storing and retrieving data from Redis
#[derive(Clone)]
pub struct Cache {
    redis_client: Client,
    write_tx: mpsc::UnboundedSender<CacheWriteMessage>,
}

/// Handle for gracefully shutting down the cache writer
pub struct CacheWriterHandle {
    shutdown_tx: mpsc::Sender<()>,
}

impl CacheWriterHandle {
    /// Initiates a graceful shutdown of the cache writer
    ///
    /// Sends a shutdown signal to the writer task and waits for it to flush
    /// all pending writes to Redis.
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(()).await;
        tracing::info!("Cache writer shutdown signal sent");
    }
}

impl Cache {
    /// Creates a new Cache instance with an async write background task
    ///
    /// This spawns a background task that processes cache writes asynchronously,
    /// preventing cache operations from blocking API responses.
    pub async fn new(redis_client: Client) -> (Self, CacheWriterHandle) {
        let (write_tx, write_rx) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

        // Spawn background task to process cache writes
        let client = redis_client.clone();
        tokio::spawn(async move {
            Self::cache_writer_task(client, write_rx, shutdown_rx).await;
        });

        let cache = Self {
            redis_client,
            write_tx,
        };

        let handle = CacheWriterHandle { shutdown_tx };

        (cache, handle)
    }

    /// Background task that processes cache write messages
    ///
    /// Continuously receives cache write requests from the channel and writes them
    /// to Redis. On shutdown signal, flushes all remaining messages before exiting.
    async fn cache_writer_task(
        client: Client,
        mut write_rx: mpsc::UnboundedReceiver<CacheWriteMessage>,
        mut shutdown_rx: mpsc::Receiver<()>,
    ) {
        tracing::info!("Cache writer task started");
        let mut pending_writes = 0;

        loop {
            tokio::select! {
                // Process write messages
                Some(msg) = write_rx.recv() => {
                    pending_writes += 1;
                    if let Err(e) = Self::write_to_redis(&client, msg).await {
                        tracing::error!(error = %e, "Failed to write to Redis cache");
                    } else {
                        pending_writes -= 1;
                    }
                }
                // Shutdown signal received
                _ = shutdown_rx.recv() => {
                    tracing::info!(pending = pending_writes, "Cache writer shutting down, flushing remaining writes");

                    // Flush all remaining messages
                    while let Some(msg) = write_rx.recv().await {
                        if let Err(e) = Self::write_to_redis(&client, msg).await {
                            tracing::error!(error = %e, "Failed to flush cache write during shutdown");
                        }
                    }

                    tracing::info!("Cache writer task stopped");
                    break;
                }
            }
        }
    }

    /// Writes a single message to Redis
    async fn write_to_redis(client: &Client, msg: CacheWriteMessage) -> AppResult<()> {
        let mut conn = client.get_multiplexed_async_connection().await?;
        let _: () = conn.set_ex(msg.key, msg.value, msg.ttl).await?;
        Ok(())
    }

    /// Retrieves a value from the cache by key
    ///
    /// This function attempts to retrieve a cached value associated with the given key.
    /// If the key exists in the cache, the value is deserialized and returned.
    /// If the key does not exist, `None` is returned.
    pub async fn get_from_cache<T: serde::de::DeserializeOwned>(
        &self,
        key: &CacheKey,
    ) -> AppResult<Option<T>> {
        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;
        let cached: Option<String> = conn.get(format!("{}", key)).await?;

        match cached {
            Some(json) => {
                let data = serde_json::from_str(&json).map_err(|e| {
                    AppError::Internal(format!("Cache deserialization error: {}", e))
                })?;
                Ok(Some(data))
            }
            None => Ok(None),
        }
    }

    /// Stores a value in the cache asynchronously without blocking
    ///
    /// This function serializes the value and sends it to a background worker
    /// via a channel. The actual Redis write happens asynchronously, so this
    /// method returns immediately without waiting for the write to complete.
    ///
    /// Use this method when you don't need confirmation that the write succeeded
    /// and want to maximize API response performance.
    pub fn set_in_background<T: serde::Serialize>(&self, key: &CacheKey, value: &T, ttl: u64) {
        let json = match serde_json::to_string(value) {
            Ok(j) => j,
            Err(e) => {
                tracing::error!(error = %e, "Cache serialization error");
                return;
            }
        };

        let msg = CacheWriteMessage {
            key: format!("{}", key),
            value: json,
            ttl,
        };

        if let Err(e) = self.write_tx.send(msg) {
            tracing::error!(error = %e, "Failed to send cache write message");
        }
    }
}

// TODO : Clean up tests to use a mock Redis server like 'mock-redis-server' crate

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_display_title_search() {
        let key = CacheKey::TitleSearch("Inception".to_string());
        assert_eq!(format!("{}", key), "search:inception");
    }

    #[test]
    fn test_cache_key_display_title_search_lowercase() {
        let key = CacheKey::TitleSearch("THE MATRIX".to_string());
        assert_eq!(format!("{}", key), "search:the matrix");
    }

    #[test]
    fn test_cache_key_display_availability() {
        let key = CacheKey::Availability("tt1375666".to_string());
        assert_eq!(format!("{}", key), "avail:tt1375666");
    }

    #[test]
    fn test_cache_key_display_availability_watchmode() {
        let key = CacheKey::Availability("3173903".to_string());
        assert_eq!(format!("{}", key), "avail:3173903");
    }

    #[test]
    fn test_cache_key_display_imdb_to_watchmode() {
        let key = CacheKey::ImdbToWatchmode("tt1375666".to_string());
        assert_eq!(format!("{}", key), "imdb2wm:tt1375666");
    }

    #[tokio::test]
    async fn test_cache_miss() {
        let redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());

        let client = create_redis_client(&redis_url).unwrap();
        let (cache, _handle) = Cache::new(client).await;

        let key = CacheKey::TitleSearch("nonexistent_key_12345".to_string());
        let retrieved: Option<Vec<String>> = cache.get_from_cache(&key).await.unwrap();

        assert_eq!(retrieved, None);
    }

    #[tokio::test]
    async fn test_set_in_background_writes_to_cache() {
        let redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());

        let client = create_redis_client(&redis_url).unwrap();
        let (cache, _handle) = Cache::new(client.clone()).await;

        let key = CacheKey::TitleSearch("test_async_write".to_string());
        let value = vec!["item1".to_string(), "item2".to_string()];

        // Write using async method (non-blocking)
        cache.set_in_background(&key, &value, 60);

        // Give the background task time to process
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Verify it was written
        let retrieved: Option<Vec<String>> = cache.get_from_cache(&key).await.unwrap();
        assert_eq!(retrieved, Some(value));

        // Clean up
        let mut conn = client.get_multiplexed_async_connection().await.unwrap();
        let _: () = conn.del(format!("{}", key)).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_in_background_multiple_writes() {
        let redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());

        let client = create_redis_client(&redis_url).unwrap();
        let (cache, _handle) = Cache::new(client.clone()).await;

        // Write multiple values asynchronously
        let keys_values = vec![
            (
                CacheKey::TitleSearch("async_test_1".to_string()),
                vec!["a".to_string()],
            ),
            (
                CacheKey::TitleSearch("async_test_2".to_string()),
                vec!["b".to_string()],
            ),
            (
                CacheKey::TitleSearch("async_test_3".to_string()),
                vec!["c".to_string()],
            ),
        ];

        for (key, value) in &keys_values {
            cache.set_in_background(key, value, 60);
        }

        // Give the background task time to process all writes
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        // Verify all were written
        for (key, expected_value) in &keys_values {
            let retrieved: Option<Vec<String>> = cache.get_from_cache(key).await.unwrap();
            assert_eq!(retrieved.as_ref(), Some(expected_value));
        }

        // Clean up
        let mut conn = client.get_multiplexed_async_connection().await.unwrap();
        for (key, _) in keys_values {
            let _: () = conn.del(format!("{}", key)).await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_cache_writer_graceful_shutdown() {
        let redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());

        let client = create_redis_client(&redis_url).unwrap();
        let (cache, handle) = Cache::new(client.clone()).await;

        let key = CacheKey::TitleSearch("test_shutdown".to_string());
        let value = vec!["shutdown_test".to_string()];

        // Write using async method
        cache.set_in_background(&key, &value, 60);

        // Trigger graceful shutdown
        handle.shutdown().await;

        // Give a moment for shutdown to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Verify the write completed before shutdown
        let retrieved: Option<Vec<String>> = cache.get_from_cache(&key).await.unwrap();
        assert_eq!(retrieved, Some(value));

        // Clean up
        let mut conn = client.get_multiplexed_async_connection().await.unwrap();
        let _: () = conn.del(format!("{}", key)).await.unwrap();
    }
}
