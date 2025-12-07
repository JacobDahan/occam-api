/// Streaming Availability API provider (via RapidAPI)
///
/// This is the current provider - keeps existing implementation but wrapped in the
/// StreamingProvider trait for easy swapping.
use crate::{
    cached,
    db::{Cache, CacheKey},
    error::{AppError, AppResult},
    models::{
        ApiShow, ApiShowDetails, AvailabilityType, ServiceAvailability, StreamingAvailability,
        Title, TitleId,
    },
    services::providers::StreamingProvider,
};
use chrono::Utc;
use reqwest::Client as HttpClient;
use serde::Deserialize;

const TITLE_CACHE_TTL: u64 = 3600; // 1 hour
const AVAIL_CACHE_TTL: u64 = 604800; // 1 week
const SEARCH_COUNTRY: &str = "us";

#[derive(Debug, Deserialize)]
struct ApiSearchResponse(Vec<ApiShow>);

#[derive(Clone)]
pub struct StreamingAvailabilityProvider {
    http_client: HttpClient,
    api_key: String,
    api_url: String,
    cache: Cache,
}

impl StreamingAvailabilityProvider {
    pub fn new(cache: Cache, api_key: String, api_url: String) -> Self {
        Self {
            http_client: HttpClient::new(),
            api_key,
            api_url,
            cache,
        }
    }

    fn convert_api_response(&self, details: ApiShowDetails) -> AppResult<StreamingAvailability> {
        let imdb_id = details
            .imdb_id
            .ok_or_else(|| AppError::ExternalApi("API response missing IMDB ID".to_string()))?;

        let mut services = Vec::new();

        if let Some(us_options) = details.streaming_options.get("us") {
            for option in us_options {
                let availability_type = match option.availability_type.as_str() {
                    "subscription" => AvailabilityType::Subscription,
                    "rent" => AvailabilityType::Rent,
                    "buy" => AvailabilityType::Buy,
                    "free" => AvailabilityType::Free,
                    "addon" => AvailabilityType::Addon,
                    _ => continue,
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
            id: TitleId::Imdb(imdb_id),
            services,
            cached_at: Utc::now(),
        })
    }
}

#[async_trait::async_trait]
impl StreamingProvider for StreamingAvailabilityProvider {
    async fn search_titles(&self, query: &str) -> AppResult<Vec<Title>> {
        if query.trim().is_empty() {
            return Err(AppError::InvalidInput(
                "Search query cannot be empty".to_string(),
            ));
        }

        cached!(
            self.cache,
            CacheKey::TitleSearch(query.to_string()),
            TITLE_CACHE_TTL,
            async move {
                // Fetch from API
                let url = format!("{}/shows/search/title", self.api_url);
                let response = self
                    .http_client
                    .get(&url)
                    .header("X-RapidAPI-Key", &self.api_key)
                    .query(&[("title", query), ("country", SEARCH_COUNTRY)])
                    .send()
                    .await?;

                if !response.status().is_success() {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    return Err(AppError::ExternalApi(format!(
                        "API returned status {}: {}",
                        status, body
                    )));
                }

                let shows: ApiSearchResponse = response.json().await?;
                let titles: Vec<Title> = shows.0.into_iter().map(Title::from).collect();

                tracing::info!(
                    query = %query,
                    results = titles.len(),
                    provider = "streaming_availability",
                    "Title search completed"
                );

                Ok(titles)
            }
        )
    }

    async fn fetch_availability(&self, title_id: &TitleId) -> AppResult<StreamingAvailability> {
        cached!(
            self.cache,
            CacheKey::Availability(format!("{}", title_id)),
            AVAIL_CACHE_TTL,
            async move {
                // Fetch from API
                let url = format!("{}/shows/{}", self.api_url, title_id);
                let response = self
                    .http_client
                    .get(&url)
                    .header("X-RapidAPI-Key", &self.api_key)
                    .query(&[("country", "us")]) // TODO: Add support for additional regions
                    .send()
                    .await?;

                if !response.status().is_success() {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    return Err(AppError::ExternalApi(format!(
                        "API returned status {}: {}",
                        status, body
                    )));
                }

                let show_details: ApiShowDetails = response.json().await?;
                let availability = self.convert_api_response(show_details)?;

                tracing::info!(
                    title_id = %title_id,
                    services = availability.services.len(),
                    provider = "streaming_availability",
                    "Availability fetched"
                );

                Ok(availability)
            }
        )
    }

    fn clone_for_task(&self) -> Box<dyn StreamingProvider> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ApiStreamingOption;
    use std::collections::HashMap;

    async fn create_test_provider() -> StreamingAvailabilityProvider {
        StreamingAvailabilityProvider {
            http_client: reqwest::Client::new(),
            api_key: "test_key".to_string(),
            api_url: "http://test.local".to_string(),
            cache: Cache::new(redis::Client::open("redis://localhost:6379").unwrap())
                .await
                .0,
        }
    }

    #[tokio::test]
    async fn test_convert_api_response_success() {
        let provider = create_test_provider().await;

        let mut streaming_options = HashMap::new();
        streaming_options.insert(
            "us".to_string(),
            vec![ApiStreamingOption {
                service: crate::models::ApiService {
                    id: "netflix".to_string(),
                    name: "Netflix".to_string(),
                },
                availability_type: "subscription".to_string(),
                quality: Some("4K".to_string()),
                link: Some("https://netflix.com/title/123".to_string()),
            }],
        );

        let details = ApiShowDetails {
            imdb_id: Some("tt1375666".to_string()),
            streaming_options,
        };

        let result = provider.convert_api_response(details).unwrap();

        assert_eq!(result.id, TitleId::Imdb("tt1375666".to_string()));
        assert_eq!(result.services.len(), 1);
        assert_eq!(result.services[0].service_id, "netflix");
        assert_eq!(result.services[0].service_name, "Netflix");
        assert_eq!(
            result.services[0].availability_type,
            AvailabilityType::Subscription
        );
        assert_eq!(result.services[0].quality, Some("4K".to_string()));
    }

    #[tokio::test]
    async fn test_convert_api_response_missing_imdb_id() {
        let provider = create_test_provider().await;

        let details = ApiShowDetails {
            imdb_id: None,
            streaming_options: HashMap::new(),
        };

        let result = provider.convert_api_response(details);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_convert_api_response_filters_availability_types() {
        let provider = create_test_provider().await;

        let mut streaming_options = HashMap::new();
        streaming_options.insert(
            "us".to_string(),
            vec![
                ApiStreamingOption {
                    service: crate::models::ApiService {
                        id: "netflix".to_string(),
                        name: "Netflix".to_string(),
                    },
                    availability_type: "subscription".to_string(),
                    quality: Some("HD".to_string()),
                    link: None,
                },
                ApiStreamingOption {
                    service: crate::models::ApiService {
                        id: "itunes".to_string(),
                        name: "iTunes".to_string(),
                    },
                    availability_type: "rent".to_string(),
                    quality: Some("HD".to_string()),
                    link: None,
                },
                ApiStreamingOption {
                    service: crate::models::ApiService {
                        id: "vudu".to_string(),
                        name: "Vudu".to_string(),
                    },
                    availability_type: "buy".to_string(),
                    quality: Some("HD".to_string()),
                    link: None,
                },
            ],
        );

        let details = ApiShowDetails {
            imdb_id: Some("tt1375666".to_string()),
            streaming_options,
        };

        let result = provider.convert_api_response(details).unwrap();

        assert_eq!(result.services.len(), 3);
        assert_eq!(
            result.services[0].availability_type,
            AvailabilityType::Subscription
        );
        assert_eq!(result.services[1].availability_type, AvailabilityType::Rent);
        assert_eq!(result.services[2].availability_type, AvailabilityType::Buy);
    }
}
