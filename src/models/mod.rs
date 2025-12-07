use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Display};

/// Identifier for a title, which can be either IMDB ID or provider-specific ID
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TitleId {
    /// IMDB ID (e.g., "tt13406094")
    Imdb(String),
    /// Watchmode-specific ID
    Watchmode(u64),
}

impl Display for TitleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TitleId::Imdb(id) => write!(f, "{}", id),
            TitleId::Watchmode(id) => write!(f, "{}", id),
        }
    }
}

/// Represents a movie or TV show title returned to the client
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Title {
    pub id: TitleId,
    pub title: String,
    pub title_type: TitleType,
    pub release_year: Option<i32>,
    pub overview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TitleType {
    Movie,
    Series,
}

// ============================================================================
// Streaming Availability API Types
// ============================================================================

/// Raw API response from Streaming Availability API
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiShow {
    pub id: String,
    #[serde(default)]
    pub imdb_id: Option<String>,
    pub title: String,
    pub show_type: String,
    #[serde(default)]
    pub overview: Option<String>,
    #[serde(default)]
    pub release_year: Option<i32>,
    #[serde(default)]
    pub first_air_year: Option<i32>,
}

impl From<ApiShow> for Title {
    fn from(show: ApiShow) -> Self {
        let title_type = match show.show_type.as_str() {
            "movie" => TitleType::Movie,
            "series" => TitleType::Series,
            _ => TitleType::Movie,
        };

        // Prefer IMDB ID if available, otherwise use the API's ID
        let id = if let Some(imdb_id) = show.imdb_id {
            TitleId::Imdb(imdb_id)
        } else {
            // Streaming Availability API uses numeric IDs - treat as string
            TitleId::Imdb(show.id)
        };

        Title {
            id,
            title: show.title,
            title_type,
            release_year: show.release_year.or(show.first_air_year),
            overview: show.overview,
        }
    }
}

/// Represents a streaming service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingService {
    pub id: String,
    pub name: String,
    pub monthly_cost: f64,
}

/// Request to find optimal streaming services
#[derive(Debug, Deserialize)]
pub struct OptimizationRequest {
    pub must_have: Vec<TitleId>,
    pub nice_to_have: Vec<TitleId>,
}

/// Response with ordered list of streaming service configurations
#[derive(Debug, Serialize)]
pub struct OptimizationResponse {
    /// Ordered list of service configurations from most preferred to least preferred
    /// First configuration is the optimal (cost-focused) solution
    /// Subsequent configurations trade cost for better nice-to-have coverage
    pub configurations: Vec<ServiceConfiguration>,
    /// Titles that are unavailable on any streaming service
    pub unavailable_must_have: Vec<TitleId>,
    pub unavailable_nice_to_have: Vec<TitleId>,
}

/// A single streaming service configuration with coverage and cost information
#[derive(Debug, Serialize, Clone)]
pub struct ServiceConfiguration {
    pub services: Vec<StreamingService>,
    pub total_cost: f64,
    pub must_have_coverage: usize,
    pub nice_to_have_coverage: usize,
}

/// Streaming availability data for a single title
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingAvailability {
    pub id: TitleId,
    pub services: Vec<ServiceAvailability>,
    pub cached_at: DateTime<Utc>,
}

/// Availability details for one service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAvailability {
    pub service_id: String,
    pub service_name: String,
    pub availability_type: AvailabilityType,
    pub quality: Option<String>,
    pub link: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AvailabilityType {
    Subscription,
    Rent,
    Buy,
    Free,
    Addon,
}

/// API response from GET /shows/{id}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiShowDetails {
    #[serde(default)]
    pub imdb_id: Option<String>,
    #[serde(default)]
    pub streaming_options: HashMap<String, Vec<ApiStreamingOption>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiStreamingOption {
    pub service: ApiService,
    #[serde(rename = "type")]
    pub availability_type: String,
    #[serde(default)]
    pub quality: Option<String>,
    #[serde(default)]
    pub link: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiService {
    pub id: String,
    pub name: String,
}

// ============================================================================
// Watchmode API Types
// ============================================================================

/// Watchmode autocomplete search result
#[derive(Debug, Clone, Deserialize)]
pub struct WatchmodeTitle {
    pub id: u64,
    pub name: String,
    #[serde(rename = "type")]
    pub title_type: String,
    pub year: Option<u32>,
    #[serde(default)]
    #[allow(dead_code)] // May be used in future for IMDB fallback
    pub imdb_id: Option<String>,
}

impl From<WatchmodeTitle> for Title {
    fn from(watchmode: WatchmodeTitle) -> Self {
        let title_type = match watchmode.title_type.as_str() {
            "movie" => TitleType::Movie,
            "tv_series" => TitleType::Series,
            _ => TitleType::Movie,
        };

        // Prefer Watchmode ID as it's cheaper to look up availability with
        let id = TitleId::Watchmode(watchmode.id);

        Title {
            id,
            title: watchmode.name,
            title_type,
            release_year: watchmode.year.map(|y| y as i32),
            overview: None,
        }
    }
}

/// Watchmode title details response
#[derive(Debug, Deserialize)]
pub struct WatchmodeTitleDetails {
    pub sources: Option<Vec<WatchmodeSource>>,
}

/// Watchmode streaming source
#[derive(Debug, Deserialize)]
pub struct WatchmodeSource {
    pub source_id: u64,
    pub name: String,
    pub web_url: Option<String>,
    #[serde(rename = "type")]
    pub source_type: String,
    pub format: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_title_id_display_imdb() {
        let id = TitleId::Imdb("tt1375666".to_string());
        assert_eq!(format!("{}", id), "tt1375666");
    }

    #[test]
    fn test_title_id_display_watchmode() {
        let id = TitleId::Watchmode(3173903);
        assert_eq!(format!("{}", id), "3173903");
    }

    #[test]
    fn test_title_id_serde_imdb() {
        let id = TitleId::Imdb("tt1375666".to_string());
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, r#"{"Imdb":"tt1375666"}"#);

        let deserialized: TitleId = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, id);
    }

    #[test]
    fn test_title_id_serde_watchmode() {
        let id = TitleId::Watchmode(3173903);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, r#"{"Watchmode":3173903}"#);

        let deserialized: TitleId = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, id);
    }

    #[test]
    fn test_api_show_to_title_with_imdb_id() {
        let api_show = ApiShow {
            id: "12345".to_string(),
            imdb_id: Some("tt1375666".to_string()),
            title: "Inception".to_string(),
            show_type: "movie".to_string(),
            overview: Some("A thief who steals corporate secrets".to_string()),
            release_year: Some(2010),
            first_air_year: None,
        };

        let title: Title = api_show.into();
        assert_eq!(title.id, TitleId::Imdb("tt1375666".to_string()));
        assert_eq!(title.title, "Inception");
        assert_eq!(title.title_type, TitleType::Movie);
        assert_eq!(title.release_year, Some(2010));
    }

    #[test]
    fn test_api_show_to_title_without_imdb_id() {
        let api_show = ApiShow {
            id: "12345".to_string(),
            imdb_id: None,
            title: "Unknown Movie".to_string(),
            show_type: "series".to_string(),
            overview: None,
            release_year: None,
            first_air_year: Some(2020),
        };

        let title: Title = api_show.into();
        assert_eq!(title.id, TitleId::Imdb("12345".to_string()));
        assert_eq!(title.title, "Unknown Movie");
        assert_eq!(title.title_type, TitleType::Series);
        assert_eq!(title.release_year, Some(2020));
    }

    #[test]
    fn test_watchmode_title_to_title_prefers_watchmode_id() {
        let watchmode_title = WatchmodeTitle {
            id: 3173903,
            name: "Inception".to_string(),
            title_type: "movie".to_string(),
            year: Some(2010),
            imdb_id: Some("tt1375666".to_string()),
        };

        let title: Title = watchmode_title.into();
        // Should prefer Watchmode ID even when IMDB ID is available (cheaper lookups)
        assert_eq!(title.id, TitleId::Watchmode(3173903));
        assert_eq!(title.title, "Inception");
        assert_eq!(title.title_type, TitleType::Movie);
        assert_eq!(title.release_year, Some(2010));
        assert_eq!(title.overview, None);
    }

    #[test]
    fn test_watchmode_title_to_title_series() {
        let watchmode_title = WatchmodeTitle {
            id: 9876543,
            name: "Obscure Series".to_string(),
            title_type: "tv_series".to_string(),
            year: Some(2021),
            imdb_id: None,
        };

        let title: Title = watchmode_title.into();
        assert_eq!(title.id, TitleId::Watchmode(9876543));
        assert_eq!(title.title, "Obscure Series");
        assert_eq!(title.title_type, TitleType::Series);
        assert_eq!(title.release_year, Some(2021));
    }
}
