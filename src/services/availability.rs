use crate::{
    error::{AppError, AppResult},
    models::{ApiShowDetails, AvailabilityType, ServiceAvailability, StreamingAvailability},
};
use chrono::Utc;
use redis::{AsyncCommands, Client as RedisClient};
use reqwest::Client as HttpClient;

const CACHE_TTL: u64 = 604800; // 1 week in seconds
const MONTHLY_QUOTA: u32 = 25_000;
const DAILY_SAFE_LIMIT: u32 = 800;

/// Service for fetching and caching streaming availability data
pub struct AvailabilityService {
    http_client: HttpClient,
    redis_client: RedisClient,
    api_key: String,
    api_url: String,
}

impl AvailabilityService {
    pub fn new(redis_client: RedisClient, api_key: String, api_url: String) -> Self {
        Self {
            http_client: HttpClient::new(),
            redis_client,
            api_key,
            api_url,
        }
    }

    /// Fetches availability data for multiple titles in parallel
    pub async fn fetch_availability_batch(
        &self,
        imdb_ids: Vec<String>,
    ) -> AppResult<Vec<StreamingAvailability>> {
        tracing::info!(title_count = imdb_ids.len(), "Fetching availability batch");

        let mut tasks = Vec::new();

        // Spawn parallel tasks for each IMDB ID
        for imdb_id in imdb_ids {
            let service = self.clone_for_task();
            let task = tokio::spawn(async move { service.fetch_single_title(&imdb_id).await });
            tasks.push(task);
        }

        // Collect results
        let mut results = Vec::new();
        let mut errors = Vec::new();

        for task in tasks {
            match task.await {
                Ok(Ok(availability)) => results.push(availability),
                Ok(Err(e)) => errors.push(e),
                Err(e) => errors.push(AppError::Internal(e.to_string())),
            }
        }

        // Log errors but don't fail the entire request if we got some results
        if !errors.is_empty() {
            tracing::warn!(
                success_count = results.len(),
                error_count = errors.len(),
                "Partial availability fetch failure"
            );
        }

        if results.is_empty() && !errors.is_empty() {
            return Err(AppError::ExternalApi(
                "Failed to fetch any availability data".to_string(),
            ));
        }

        tracing::info!(fetched = results.len(), "Availability data fetched");

        Ok(results)
    }

    /// Fetches availability for a single title (checks cache first)
    async fn fetch_single_title(&self, imdb_id: &str) -> AppResult<StreamingAvailability> {
        // Check Redis cache
        if let Some(cached) = self.get_from_redis(imdb_id).await? {
            tracing::debug!(imdb_id = %imdb_id, "Cache hit");
            return Ok(cached);
        }

        tracing::debug!(imdb_id = %imdb_id, "Cache miss");

        // Cache miss - fetch from API
        let availability = self.call_api(imdb_id).await?;

        // Store in Redis
        self.store_in_redis(&availability).await?;

        Ok(availability)
    }

    /// Attempts to retrieve cached availability from Redis
    async fn get_from_redis(&self, imdb_id: &str) -> AppResult<Option<StreamingAvailability>> {
        let cache_key = format!("avail:{}", imdb_id);
        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;

        let cached: Option<String> = conn.get(&cache_key).await.map_err(|e| {
            tracing::warn!(error = %e, "Redis get failed");
            e
        })?;

        match cached {
            Some(json) => {
                let availability: StreamingAvailability =
                    serde_json::from_str(&json).map_err(|e| {
                        AppError::Internal(format!("Cache deserialization error: {}", e))
                    })?;
                Ok(Some(availability))
            }
            None => Ok(None),
        }
    }

    /// Stores availability data in Redis cache
    async fn store_in_redis(&self, data: &StreamingAvailability) -> AppResult<()> {
        let cache_key = format!("avail:{}", data.imdb_id);
        let json = serde_json::to_string(data)
            .map_err(|e| AppError::Internal(format!("Cache serialization error: {}", e)))?;

        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;

        let _: () = conn
            .set_ex(&cache_key, json, CACHE_TTL)
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "Redis set failed");
                e
            })?;

        tracing::debug!(imdb_id = %data.imdb_id, ttl = CACHE_TTL, "Cached availability");

        Ok(())
    }

    /// Calls the Streaming Availability API
    async fn call_api(&self, imdb_id: &str) -> AppResult<StreamingAvailability> {
        // Check rate limit before calling
        self.check_rate_limit().await?;

        let url = format!("{}/shows/{}", self.api_url, imdb_id);

        tracing::debug!(imdb_id = %imdb_id, "Fetching from external API");

        let response = self
            .http_client
            .get(&url)
            .header("X-RapidAPI-Key", &self.api_key)
            .query(&[("country", "us")])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            tracing::error!(
                imdb_id = %imdb_id,
                status = %status,
                body = %body,
                "External API request failed"
            );
            return Err(AppError::ExternalApi(format!(
                "API returned status {}: {}",
                status, body
            )));
        }

        let show_details: ApiShowDetails = response.json().await?;

        // Increment usage counter after successful call
        self.increment_api_usage().await?;

        // Convert API response to our model
        let availability = self.convert_api_response(show_details)?;

        tracing::info!(
            imdb_id = %imdb_id,
            service_count = availability.services.len(),
            "Successfully fetched availability from API"
        );

        tracing::debug!(
            imdb_id = %imdb_id,
            services = ?availability.services,
            "Availability details"
        );

        Ok(availability)
    }

    /// Converts API response to StreamingAvailability
    fn convert_api_response(&self, details: ApiShowDetails) -> AppResult<StreamingAvailability> {
        let imdb_id = details
            .imdb_id
            .ok_or_else(|| AppError::ExternalApi("API response missing IMDB ID".to_string()))?;

        let mut services = Vec::new();

        // streaming_options is a HashMap<country_code, Vec<ApiStreamingOption>>
        // We're querying for "us" so we look for that key
        if let Some(us_options) = details.streaming_options.get("us") {
            for option in us_options {
                let availability_type = match option.availability_type.as_str() {
                    "subscription" => AvailabilityType::Subscription,
                    "rent" => AvailabilityType::Rent,
                    "buy" => AvailabilityType::Buy,
                    "free" => AvailabilityType::Free,
                    "addon" => AvailabilityType::Addon,
                    _ => continue, // Skip unknown types
                };

                services.push(ServiceAvailability {
                    service_id: option.service.id.clone(),
                    service_name: option.service.name.clone(),
                    availability_type,
                    quality: option.quality.clone(),
                    link: option.link.clone(),
                });
            }
        }

        Ok(StreamingAvailability {
            imdb_id,
            services,
            cached_at: Utc::now(),
        })
    }

    /// Checks if we're within API rate limits
    async fn check_rate_limit(&self) -> AppResult<bool> {
        let month_key = format!("api_usage:{}", Utc::now().format("%Y-%m"));
        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;

        let count: u32 = conn.get(&month_key).await.unwrap_or(0);

        if count >= MONTHLY_QUOTA {
            tracing::error!(
                current = count,
                quota = MONTHLY_QUOTA,
                "Monthly API quota exceeded"
            );
            return Err(AppError::ExternalApi(
                "API quota exceeded for this month".to_string(),
            ));
        }

        // Log warning at 80% usage
        if count as f32 / MONTHLY_QUOTA as f32 > 0.8 {
            tracing::warn!(
                current = count,
                quota = MONTHLY_QUOTA,
                remaining = MONTHLY_QUOTA - count,
                "API quota at 80%"
            );
        }

        Ok(true)
    }

    /// Increments API usage counters
    async fn increment_api_usage(&self) -> AppResult<()> {
        let month_key = format!("api_usage:{}", Utc::now().format("%Y-%m"));
        let day_key = format!("api_usage:daily:{}", Utc::now().format("%Y-%m-%d"));

        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;

        // Increment monthly counter
        let _: () = conn.incr(&month_key, 1).await?;

        // Set expiration at end of next month (conservative)
        let _: () = conn.expire(&month_key, 60 * 60 * 24 * 32).await?;

        // Increment daily counter
        let _: () = conn.incr(&day_key, 1).await?;
        let _: () = conn.expire(&day_key, 604800).await?; // 7 days

        let count: u32 = conn.get(&month_key).await.unwrap_or(0);
        tracing::debug!(
            monthly_count = count,
            quota_remaining = MONTHLY_QUOTA - count,
            "API usage incremented"
        );

        Ok(())
    }

    /// Helper to clone service for parallel tasks
    fn clone_for_task(&self) -> Self {
        Self {
            http_client: self.http_client.clone(),
            redis_client: self.redis_client.clone(),
            api_key: self.api_key.clone(),
            api_url: self.api_url.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ApiPrice, ApiService};
    use std::collections::HashMap;

    // Helper to create a service instance for testing (no real Redis needed)
    fn create_test_service() -> AvailabilityService {
        // Use a dummy Redis URL - we won't actually connect in these tests
        AvailabilityService {
            http_client: reqwest::Client::new(),
            redis_client: redis::Client::open("redis://127.0.0.1").unwrap(),
            api_key: "test_key".to_string(),
            api_url: "test_url".to_string(),
        }
    }

    #[test]
    fn test_convert_api_response_success() {
        let service = create_test_service();

        let mut streaming_options = HashMap::new();
        streaming_options.insert(
            "us".to_string(),
            vec![
                crate::models::ApiStreamingOption {
                    service: ApiService {
                        id: "netflix".to_string(),
                        name: "Netflix".to_string(),
                    },
                    availability_type: "subscription".to_string(),
                    price: Some(ApiPrice {
                        amount: Some("15.49".to_string()),
                        currency: Some("USD".to_string()),
                    }),
                    quality: Some("4k".to_string()),
                    link: Some("https://netflix.com/watch/123".to_string()),
                },
                crate::models::ApiStreamingOption {
                    service: ApiService {
                        id: "hulu".to_string(),
                        name: "Hulu".to_string(),
                    },
                    availability_type: "subscription".to_string(),
                    price: None,
                    quality: Some("hd".to_string()),
                    link: None,
                },
            ],
        );

        let api_response = crate::models::ApiShowDetails {
            id: "show123".to_string(),
            imdb_id: Some("tt1234567".to_string()),
            title: "Test Show".to_string(),
            show_type: "series".to_string(),
            streaming_options,
        };

        let result = service.convert_api_response(api_response).unwrap();

        assert_eq!(result.imdb_id, "tt1234567");
        assert_eq!(result.services.len(), 2);

        let netflix = &result.services[0];
        assert_eq!(netflix.service_id, "netflix");
        assert_eq!(netflix.service_name, "Netflix");
        assert_eq!(netflix.availability_type, AvailabilityType::Subscription);
        assert_eq!(netflix.quality, Some("4k".to_string()));

        let hulu = &result.services[1];
        assert_eq!(hulu.service_id, "hulu");
    }

    #[test]
    fn test_convert_api_response_missing_imdb_id() {
        let service = create_test_service();

        let api_response = crate::models::ApiShowDetails {
            id: "show123".to_string(),
            imdb_id: None,
            title: "Test Show".to_string(),
            show_type: "movie".to_string(),
            streaming_options: HashMap::new(),
        };

        let result = service.convert_api_response(api_response);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing IMDB ID"));
    }

    #[test]
    fn test_convert_api_response_filters_unknown_types() {
        let service = create_test_service();

        let mut streaming_options = HashMap::new();
        streaming_options.insert(
            "us".to_string(),
            vec![
                crate::models::ApiStreamingOption {
                    service: ApiService {
                        id: "netflix".to_string(),
                        name: "Netflix".to_string(),
                    },
                    availability_type: "subscription".to_string(),
                    price: None,
                    quality: None,
                    link: None,
                },
                crate::models::ApiStreamingOption {
                    service: ApiService {
                        id: "unknown".to_string(),
                        name: "Unknown".to_string(),
                    },
                    availability_type: "unknown_type".to_string(),
                    price: None,
                    quality: None,
                    link: None,
                },
            ],
        );

        let api_response = crate::models::ApiShowDetails {
            id: "show123".to_string(),
            imdb_id: Some("tt1234567".to_string()),
            title: "Test Show".to_string(),
            show_type: "movie".to_string(),
            streaming_options,
        };

        let result = service.convert_api_response(api_response).unwrap();

        // Should only include netflix, not unknown type
        assert_eq!(result.services.len(), 1);
        assert_eq!(result.services[0].service_id, "netflix");
    }

    #[test]
    fn test_convert_api_response_all_availability_types() {
        let service = create_test_service();

        let mut streaming_options = HashMap::new();
        streaming_options.insert(
            "us".to_string(),
            vec![
                crate::models::ApiStreamingOption {
                    service: ApiService {
                        id: "s1".to_string(),
                        name: "Service 1".to_string(),
                    },
                    availability_type: "subscription".to_string(),
                    price: None,
                    quality: None,
                    link: None,
                },
                crate::models::ApiStreamingOption {
                    service: ApiService {
                        id: "s2".to_string(),
                        name: "Service 2".to_string(),
                    },
                    availability_type: "rent".to_string(),
                    price: Some(ApiPrice {
                        amount: Some("3.99".to_string()),
                        currency: Some("USD".to_string()),
                    }),
                    quality: None,
                    link: None,
                },
                crate::models::ApiStreamingOption {
                    service: ApiService {
                        id: "s3".to_string(),
                        name: "Service 3".to_string(),
                    },
                    availability_type: "buy".to_string(),
                    price: Some(ApiPrice {
                        amount: Some("12.99".to_string()),
                        currency: Some("USD".to_string()),
                    }),
                    quality: None,
                    link: None,
                },
                crate::models::ApiStreamingOption {
                    service: ApiService {
                        id: "s4".to_string(),
                        name: "Service 4".to_string(),
                    },
                    availability_type: "free".to_string(),
                    price: None,
                    quality: None,
                    link: None,
                },
                crate::models::ApiStreamingOption {
                    service: ApiService {
                        id: "s5".to_string(),
                        name: "Service 5".to_string(),
                    },
                    availability_type: "addon".to_string(),
                    price: Some(ApiPrice {
                        amount: Some("5.99".to_string()),
                        currency: Some("USD".to_string()),
                    }),
                    quality: None,
                    link: None,
                },
            ],
        );

        let api_response = crate::models::ApiShowDetails {
            id: "show123".to_string(),
            imdb_id: Some("tt1234567".to_string()),
            title: "Test Show".to_string(),
            show_type: "movie".to_string(),
            streaming_options,
        };

        let result = service.convert_api_response(api_response).unwrap();

        assert_eq!(result.services.len(), 5);
        assert_eq!(
            result.services[0].availability_type,
            AvailabilityType::Subscription
        );
        assert_eq!(result.services[1].availability_type, AvailabilityType::Rent);
        assert_eq!(result.services[2].availability_type, AvailabilityType::Buy);
        assert_eq!(result.services[3].availability_type, AvailabilityType::Free);
        assert_eq!(
            result.services[4].availability_type,
            AvailabilityType::Addon
        );
    }
}
