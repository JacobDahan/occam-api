use redis::AsyncCommands;
use redis::Client;
use std::fmt::Display;

use crate::error::AppError;
use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CacheKey {
    TitleSearch(String),
    Availability(String),
}

impl Display for CacheKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheKey::TitleSearch(query) => write!(f, "search:{}", query.to_lowercase()),
            CacheKey::Availability(id) => write!(f, "avail:{}", id),
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

/// Cache handler for storing and retrieving data from Redis
#[derive(Clone)]
pub struct Cache {
    redis_client: Client,
}

impl Cache {
    pub fn new(redis_client: Client) -> Self {
        Self { redis_client }
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

    /// Stores a value in the cache with the specified key
    ///
    /// This function serializes the provided value and stores it in the cache
    /// under the given key. The cached value will expire after 1 hour.
    pub async fn set_in_cache<T: serde::Serialize>(
        &self,
        key: &CacheKey,
        value: &T,
        ttl: u64,
    ) -> AppResult<()> {
        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;
        let json = serde_json::to_string(value)
            .map_err(|e| AppError::Internal(format!("Cache serialization error: {}", e)))?;
        let _: () = conn.set_ex(format!("{}", key), json, ttl).await?;
        Ok(())
    }
}

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

    #[tokio::test]
    async fn test_cache_roundtrip() {
        // Use a test Redis instance
        let redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());

        let client = create_redis_client(&redis_url).unwrap();
        let cache = Cache::new(client);

        // Test data
        let key = CacheKey::TitleSearch("test_roundtrip".to_string());
        let value = vec!["item1".to_string(), "item2".to_string()];

        // Set in cache
        cache.set_in_cache(&key, &value, 60).await.unwrap();

        // Get from cache
        let retrieved: Option<Vec<String>> = cache.get_from_cache(&key).await.unwrap();

        assert_eq!(retrieved, Some(value));

        // Clean up
        let mut conn = cache
            .redis_client
            .get_multiplexed_async_connection()
            .await
            .unwrap();
        let _: () = conn.del(format!("{}", key)).await.unwrap();
    }

    #[tokio::test]
    async fn test_cache_miss() {
        let redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());

        let client = create_redis_client(&redis_url).unwrap();
        let cache = Cache::new(client);

        let key = CacheKey::TitleSearch("nonexistent_key_12345".to_string());
        let retrieved: Option<Vec<String>> = cache.get_from_cache(&key).await.unwrap();

        assert_eq!(retrieved, None);
    }
}
