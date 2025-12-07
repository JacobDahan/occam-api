/// Watchmode API provider
///
/// Provides both title search (autocomplete) and streaming availability data.
/// Uses Watchmode's proprietary IDs internally, but returns IMDB IDs for consistency.
///
/// API Flow:
/// 1. Title Search: /autocomplete-search/ → returns Watchmode ID + IMDB ID
/// 2. Availability: /title/{watchmode_id}/details/ → returns streaming sources
///
/// Alternatively, can use /search/ to convert IMDB ID → Watchmode ID, then fetch details.
use crate::{
    cached,
    db::{Cache, CacheKey},
    error::{AppError, AppResult},
    models::{
        AvailabilityType, ServiceAvailability, StreamingAvailability, Title, TitleId,
        WatchmodeTitle, WatchmodeTitleDetails,
    },
    services::providers::StreamingProvider,
};
use chrono::Utc;
use reqwest::Client as HttpClient;
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::HashMap;

const TITLE_CACHE_TTL: u64 = 3600; // 1 hour
const AVAIL_CACHE_TTL: u64 = 604800; // 1 week

#[derive(Clone)]
pub struct WatchmodeProvider {
    http_client: HttpClient,
    api_key: String,
    api_url: String,
    cache: Cache,
    /// Cache of Watchmode ID → (service_id, service_name) mappings
    /// Loaded once at startup from database
    service_mappings: HashMap<i32, (String, String)>,
}

impl WatchmodeProvider {
    /// Creates a new Watchmode provider and loads service mappings from database
    pub async fn new(
        cache: Cache,
        db_pool: PgPool,
        api_key: String,
        api_url: String,
    ) -> AppResult<Self> {
        // Load Watchmode service ID mappings from database
        let service_mappings = Self::load_service_mappings(&db_pool).await?;

        tracing::info!(
            mappings_count = service_mappings.len(),
            "Loaded Watchmode service mappings from database"
        );

        Ok(Self {
            http_client: HttpClient::new(),
            api_key,
            api_url,
            cache,
            service_mappings,
        })
    }

    /// Loads Watchmode service ID mappings from the database
    async fn load_service_mappings(db_pool: &PgPool) -> AppResult<HashMap<i32, (String, String)>> {
        let rows = sqlx::query!(
            r#"
            SELECT watchmode_service_id, id, name
            FROM streaming_services
            WHERE watchmode_service_id IS NOT NULL AND active = true
            "#
        )
        .fetch_all(db_pool)
        .await?;

        let mut mappings = HashMap::new();
        for row in rows {
            if let Some(watchmode_id) = row.watchmode_service_id {
                mappings.insert(watchmode_id, (row.id, row.name));
            }
        }

        Ok(mappings)
    }

    /// Convert Watchmode service ID to standard service ID using database mappings
    fn map_service_id(&self, watchmode_id: u64) -> Option<(String, String)> {
        self.service_mappings
            .get(&(watchmode_id as i32))
            .map(|(id, name)| (id.clone(), name.clone()))
    }

    /// Convert Watchmode availability type to our AvailabilityType
    fn parse_availability_type(&self, source_type: &str) -> Option<AvailabilityType> {
        match source_type.to_lowercase().as_str() {
            "sub" | "subscription" => Some(AvailabilityType::Subscription),
            "rent" => Some(AvailabilityType::Rent),
            "buy" | "purchase" => Some(AvailabilityType::Buy),
            "free" => Some(AvailabilityType::Free),
            "addon" => Some(AvailabilityType::Addon),
            _ => None,
        }
    }

    /// Lookup Watchmode ID by IMDB ID
    async fn get_watchmode_id(&self, imdb_id: &str) -> AppResult<u64> {
        let url = format!("{}/v1/search/", self.api_url);

        let response = self
            .http_client
            .get(&url)
            .query(&[
                ("apiKey", self.api_key.as_str()),
                ("search_field", "imdb_id"),
                ("search_value", imdb_id),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::ExternalApi(format!(
                "Watchmode API returned status {}: {}",
                status, body
            )));
        }

        #[derive(Deserialize)]
        struct SearchResponse {
            title_results: Vec<WatchmodeTitle>,
        }

        let search_response: SearchResponse = response.json().await?;

        search_response
            .title_results
            .first()
            .map(|r| r.id)
            .ok_or_else(|| {
                AppError::ExternalApi(format!("No Watchmode ID found for IMDB ID {}", imdb_id))
            })
    }
}

#[async_trait::async_trait]
impl StreamingProvider for WatchmodeProvider {
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
                let url = format!("{}/v1/autocomplete-search/", self.api_url);

                let response = self
                    .http_client
                    .get(&url)
                    .query(&[
                        ("apiKey", self.api_key.as_str()),
                        ("search_value", query),
                        ("search_type", "1"), // 1 = movies and TV
                    ])
                    .send()
                    .await?;

                if !response.status().is_success() {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    return Err(AppError::ExternalApi(format!(
                        "Watchmode API returned status {}: {}",
                        status, body
                    )));
                }

                let results: serde_json::Value = response.json().await?;
                let results_array = results["results"].as_array().ok_or_else(|| {
                    AppError::ExternalApi("Invalid Watchmode response format".to_string())
                })?;

                let titles: Vec<Title> = results_array
                    .iter()
                    .filter_map(|result| {
                        serde_json::from_value::<WatchmodeTitle>(result.clone())
                            .map(Title::from)
                            .ok()
                    })
                    .collect();

                tracing::info!(
                    query = %query,
                    results = titles.len(),
                    provider = "watchmode",
                    "Title search completed"
                );

                Ok(titles)
            }
        )
    }

    async fn fetch_availability(&self, title_id: &TitleId) -> AppResult<StreamingAvailability> {
        // Determine the Watchmode ID and IMDB ID based on what we have
        let watchmode_id = match title_id {
            TitleId::Watchmode(id) => *id,
            TitleId::Imdb(imdb_id) => {
                // Need to look up Watchmode ID from IMDB ID
                // TODO: Cache this mapping to reduce API calls
                self.get_watchmode_id(imdb_id).await?
            }
        };

        cached!(
            self.cache,
            CacheKey::Availability(format!("{}", title_id)),
            AVAIL_CACHE_TTL,
            async move {
                // Fetch title details with sources
                let url = format!("{}/v1/title/{}/details/", self.api_url, watchmode_id);

                let response = self
                    .http_client
                    .get(&url)
                    .query(&[
                        ("apiKey", self.api_key.as_str()),
                        ("append_to_response", "sources"),
                    ])
                    .send()
                    .await?;

                if !response.status().is_success() {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    return Err(AppError::ExternalApi(format!(
                        "Watchmode API returned status {}: {}",
                        status, body
                    )));
                }

                // Get response text for debugging
                let response_text = response.text().await?;
                tracing::debug!(response = %response_text, "Raw Watchmode API response");

                let details: WatchmodeTitleDetails =
                    serde_json::from_str(&response_text).map_err(|e| {
                        tracing::error!(
                            error = %e,
                            response = %response_text,
                            "Failed to deserialize Watchmode response"
                        );
                        AppError::ExternalApi(format!("Failed to parse Watchmode response: {}", e))
                    })?;

                // Convert sources to our format
                let mut services = Vec::new();
                if let Some(sources) = details.sources {
                    for source in sources {
                        // Map Watchmode service ID to our standard ID
                        if let Some((service_id, service_name)) =
                            self.map_service_id(source.source_id)
                        {
                            if let Some(availability_type) =
                                self.parse_availability_type(&source.source_type)
                            {
                                services.push(ServiceAvailability {
                                    service_id,
                                    service_name,
                                    availability_type,
                                    quality: source.format,
                                    link: source.web_url,
                                });
                            }
                        } else {
                            tracing::debug!(
                                watchmode_service_id = source.source_id,
                                service_name = %source.name,
                                "Unknown Watchmode service ID - update SERVICE_MAPPINGS"
                            );
                        }
                    }
                }

                let availability = StreamingAvailability {
                    id: TitleId::Watchmode(watchmode_id),
                    services,
                    cached_at: Utc::now(),
                };

                tracing::info!(
                    title_id = %watchmode_id,
                    watchmode_id = watchmode_id,
                    services = availability.services.len(),
                    provider = "watchmode",
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
    use crate::models::WatchmodeSource;
    use std::collections::HashMap;

    fn create_test_provider() -> WatchmodeProvider {
        let mut service_mappings = HashMap::new();
        service_mappings.insert(203, ("netflix".to_string(), "Netflix".to_string()));
        service_mappings.insert(157, ("hulu".to_string(), "Hulu".to_string()));
        service_mappings.insert(26, ("prime".to_string(), "Prime Video".to_string()));

        WatchmodeProvider {
            http_client: reqwest::Client::new(),
            api_key: "test_key".to_string(),
            api_url: "http://test.local".to_string(),
            cache: Cache::new(redis::Client::open("redis://localhost:6379").unwrap()),
            service_mappings,
        }
    }

    #[test]
    fn test_map_service_id_found() {
        let provider = create_test_provider();
        let result = provider.map_service_id(203);
        assert_eq!(result, Some(("netflix".to_string(), "Netflix".to_string())));
    }

    #[test]
    fn test_map_service_id_not_found() {
        let provider = create_test_provider();
        let result = provider.map_service_id(999);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_availability_type_subscription() {
        let provider = create_test_provider();
        assert_eq!(
            provider.parse_availability_type("sub"),
            Some(AvailabilityType::Subscription)
        );
        assert_eq!(
            provider.parse_availability_type("subscription"),
            Some(AvailabilityType::Subscription)
        );
    }

    #[test]
    fn test_parse_availability_type_rent() {
        let provider = create_test_provider();
        assert_eq!(
            provider.parse_availability_type("rent"),
            Some(AvailabilityType::Rent)
        );
    }

    #[test]
    fn test_parse_availability_type_buy() {
        let provider = create_test_provider();
        assert_eq!(
            provider.parse_availability_type("buy"),
            Some(AvailabilityType::Buy)
        );
        assert_eq!(
            provider.parse_availability_type("purchase"),
            Some(AvailabilityType::Buy)
        );
    }

    #[test]
    fn test_parse_availability_type_free() {
        let provider = create_test_provider();
        assert_eq!(
            provider.parse_availability_type("free"),
            Some(AvailabilityType::Free)
        );
    }

    #[test]
    fn test_parse_availability_type_addon() {
        let provider = create_test_provider();
        assert_eq!(
            provider.parse_availability_type("addon"),
            Some(AvailabilityType::Addon)
        );
    }

    #[test]
    fn test_parse_availability_type_invalid() {
        let provider = create_test_provider();
        assert_eq!(provider.parse_availability_type("unknown"), None);
    }

    #[test]
    fn test_watchmode_title_deserialization() {
        let json = r#"{
            "id": 3173903,
            "name": "Inception",
            "type": "movie",
            "year": 2010,
            "imdb_id": "tt1375666"
        }"#;

        let result: WatchmodeTitle = serde_json::from_str(json).unwrap();
        assert_eq!(result.id, 3173903);
        assert_eq!(result.name, "Inception");
        assert_eq!(result.title_type, "movie");
        assert_eq!(result.year, Some(2010));
        assert_eq!(result.imdb_id, Some("tt1375666".to_string()));
    }

    #[test]
    fn test_watchmode_source_deserialization() {
        let json = r#"{
            "source_id": 203,
            "name": "Netflix",
            "type": "sub",
            "format": "4K",
            "web_url": "https://www.netflix.com/title/70131314"
        }"#;

        let source: WatchmodeSource = serde_json::from_str(json).unwrap();
        assert_eq!(source.source_id, 203);
        assert_eq!(source.name, "Netflix");
        assert_eq!(source.source_type, "sub");
        assert_eq!(source.format, Some("4K".to_string()));
        assert_eq!(
            source.web_url,
            Some("https://www.netflix.com/title/70131314".to_string())
        );
    }
}
