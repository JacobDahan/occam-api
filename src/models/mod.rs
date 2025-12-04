use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a movie or TV show title returned to the client
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Title {
    pub id: String,
    pub imdb_id: Option<String>,
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

        Title {
            id: show.id,
            imdb_id: show.imdb_id,
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
    pub must_have: Vec<String>,
    pub nice_to_have: Vec<String>,
}

/// Response with ordered list of streaming service configurations
#[derive(Debug, Serialize)]
pub struct OptimizationResponse {
    /// Ordered list of service configurations from most preferred to least preferred
    /// First configuration is the optimal (cost-focused) solution
    /// Subsequent configurations trade cost for better nice-to-have coverage
    pub configurations: Vec<ServiceConfiguration>,
    /// Titles that are unavailable on any streaming service
    pub unavailable_must_have: Vec<String>,
    pub unavailable_nice_to_have: Vec<String>,
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
    pub imdb_id: String,
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
    pub id: String,
    #[serde(default)]
    pub imdb_id: Option<String>,
    pub title: String,
    pub show_type: String,
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
    pub price: Option<ApiPrice>,
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

#[derive(Debug, Clone, Deserialize)]
pub struct ApiPrice {
    #[serde(default)]
    pub amount: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
}
