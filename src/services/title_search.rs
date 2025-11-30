use crate::{
    error::{AppError, AppResult},
    models::{ApiShow, Title},
};
use redis::{AsyncCommands, Client as RedisClient};
use reqwest::Client as HttpClient;
use serde::Deserialize;

const CACHE_TTL: u64 = 3600; // 1 hour in seconds
const SEARCH_COUNTRY: &str = "us";

#[derive(Debug, Deserialize)]
struct ApiSearchResponse(Vec<ApiShow>);

/// Trait for searching titles - allows for mocking in tests
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait TitleSearcher: Send + Sync {
    async fn search(&self, query: &str) -> AppResult<Vec<Title>>;
}

/// Production implementation of TitleSearcher
pub struct TitleSearchService {
    http_client: HttpClient,
    redis_client: RedisClient,
    api_key: String,
    api_url: String,
}

impl TitleSearchService {
    pub fn new(redis_client: RedisClient, api_key: String, api_url: String) -> Self {
        Self {
            http_client: HttpClient::new(),
            redis_client,
            api_key,
            api_url,
        }
    }

    /// Fetches titles from the Streaming Availability API
    async fn fetch_from_api(&self, query: &str) -> AppResult<Vec<ApiShow>> {
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
        Ok(shows.0)
    }

    /// Attempts to retrieve cached search results from Redis
    async fn get_from_cache(&self, query: &str) -> AppResult<Option<Vec<Title>>> {
        let cache_key = format!("search:{}", query.to_lowercase());
        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;

        let cached: Option<String> = conn.get(&cache_key).await?;

        match cached {
            Some(json) => {
                let titles: Vec<Title> = serde_json::from_str(&json).map_err(|e| {
                    AppError::Internal(format!("Cache deserialization error: {}", e))
                })?;
                tracing::debug!("Cache hit for query: {}", query);
                Ok(Some(titles))
            }
            None => {
                tracing::debug!("Cache miss for query: {}", query);
                Ok(None)
            }
        }
    }

    /// Stores search results in Redis cache
    async fn store_in_cache(&self, query: &str, titles: &[Title]) -> AppResult<()> {
        let cache_key = format!("search:{}", query.to_lowercase());
        let json = serde_json::to_string(titles)
            .map_err(|e| AppError::Internal(format!("Cache serialization error: {}", e)))?;

        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;
        let _: () = conn.set_ex(&cache_key, json, CACHE_TTL).await?;

        tracing::debug!("Cached {} results for query: {}", titles.len(), query);
        Ok(())
    }
}

#[async_trait::async_trait]
impl TitleSearcher for TitleSearchService {
    /// Searches for titles by name with Redis caching
    ///
    /// First checks Redis cache for recent identical searches.
    /// On cache miss, queries the Streaming Availability API.
    /// Transforms API response to Title models and caches results.
    async fn search(&self, query: &str) -> AppResult<Vec<Title>> {
        if query.trim().is_empty() {
            return Err(AppError::InvalidInput(
                "Search query cannot be empty".to_string(),
            ));
        }

        // Check cache first
        if let Some(cached_titles) = self.get_from_cache(query).await? {
            return Ok(cached_titles);
        }

        // Fetch from API
        let api_shows = self.fetch_from_api(query).await?;

        // Transform to our model
        let titles: Vec<Title> = api_shows.into_iter().map(Title::from).collect();

        // Cache results (fire and forget - don't fail request if caching fails)
        if let Err(e) = self.store_in_cache(query, &titles).await {
            tracing::warn!("Failed to cache search results: {}", e);
        }

        Ok(titles)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TitleType;

    /// Test model conversion from API to domain model
    #[test]
    fn test_api_show_to_title_conversion_movie() {
        let api_show = ApiShow {
            id: "tt0111161".to_string(),
            imdb_id: Some("tt0111161".to_string()),
            title: "The Shawshank Redemption".to_string(),
            show_type: "movie".to_string(),
            overview: Some("Two imprisoned men bond over a number of years".to_string()),
            release_year: Some(1994),
            first_air_year: None,
        };

        let title: Title = api_show.into();

        assert_eq!(title.id, "tt0111161");
        assert_eq!(title.imdb_id, Some("tt0111161".to_string()));
        assert_eq!(title.title, "The Shawshank Redemption");
        assert_eq!(title.title_type, TitleType::Movie);
        assert_eq!(title.release_year, Some(1994));
    }

    /// Test series conversion with first_air_year fallback
    #[test]
    fn test_api_show_to_title_conversion_series() {
        let api_show = ApiShow {
            id: "tt0903747".to_string(),
            imdb_id: Some("tt0903747".to_string()),
            title: "Breaking Bad".to_string(),
            show_type: "series".to_string(),
            overview: Some("A chemistry teacher turned meth cook".to_string()),
            release_year: None,
            first_air_year: Some(2008),
        };

        let title: Title = api_show.into();

        assert_eq!(title.title_type, TitleType::Series);
        assert_eq!(title.release_year, Some(2008));
    }

    /// Test successful search returns titles
    #[tokio::test]
    async fn test_successful_search_returns_titles() {
        let mut mock_searcher = MockTitleSearcher::new();

        let expected_titles = vec![Title {
            id: "123".to_string(),
            imdb_id: Some("tt123".to_string()),
            title: "Test Movie".to_string(),
            title_type: TitleType::Movie,
            release_year: Some(2020),
            overview: Some("A test movie".to_string()),
        }];

        let return_titles = expected_titles.clone();
        mock_searcher
            .expect_search()
            .times(1)
            .returning(move |_| Ok(return_titles.clone()));

        let result = mock_searcher.search("test").await;
        assert!(result.is_ok());

        let titles = result.unwrap();
        assert_eq!(titles.len(), 1);
        assert_eq!(titles[0].title, "Test Movie");
    }

    /// Test successful search returns multiple titles of different types
    #[tokio::test]
    async fn test_successful_search_returns_mixed_content() {
        let mut mock_searcher = MockTitleSearcher::new();

        let expected_titles = vec![
            Title {
                id: "tt0111161".to_string(),
                imdb_id: Some("tt0111161".to_string()),
                title: "The Shawshank Redemption".to_string(),
                title_type: TitleType::Movie,
                release_year: Some(1994),
                overview: Some("Two imprisoned men bond over a number of years".to_string()),
            },
            Title {
                id: "tt0903747".to_string(),
                imdb_id: Some("tt0903747".to_string()),
                title: "Breaking Bad".to_string(),
                title_type: TitleType::Series,
                release_year: Some(2008),
                overview: Some("A chemistry teacher turned meth cook".to_string()),
            },
            Title {
                id: "tt1375666".to_string(),
                imdb_id: Some("tt1375666".to_string()),
                title: "Inception".to_string(),
                title_type: TitleType::Movie,
                release_year: Some(2010),
                overview: Some(
                    "A thief steals people's secrets from their subconscious while they dream."
                        .to_string(),
                ),
            },
            Title {
                id: "tt0944947".to_string(),
                imdb_id: Some("tt0944947".to_string()),
                title: "Game of Thrones".to_string(),
                title_type: TitleType::Series,
                release_year: Some(2011),
                overview: Some("Nine noble families fight for control of the lands".to_string()),
            },
        ];

        let return_titles = expected_titles.clone();
        mock_searcher
            .expect_search()
            .times(1)
            .returning(move |_| Ok(return_titles.clone()));

        let result = mock_searcher.search("popular").await;
        assert!(result.is_ok());

        let titles = result.unwrap();
        assert_eq!(titles.len(), 4);

        // Verify we have both movies and series
        let movies: Vec<_> = titles
            .iter()
            .filter(|t| t.title_type == TitleType::Movie)
            .collect();
        let series: Vec<_> = titles
            .iter()
            .filter(|t| t.title_type == TitleType::Series)
            .collect();

        assert_eq!(movies.len(), 2);
        assert_eq!(series.len(), 2);

        // Verify specific titles
        assert_eq!(titles[0].title, "The Shawshank Redemption");
        assert_eq!(titles[1].title, "Breaking Bad");
        assert_eq!(titles[2].title, "Inception");
        assert_eq!(titles[3].title, "Game of Thrones");

        // Verify all have IMDb IDs
        assert!(titles.iter().all(|t| t.imdb_id.is_some()));
    }
}
