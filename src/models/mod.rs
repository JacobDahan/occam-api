use serde::{Deserialize, Serialize};

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

/// Response with optimal streaming service combination
#[derive(Debug, Serialize)]
pub struct OptimizationResponse {
    pub services: Vec<StreamingService>,
    pub total_cost: f64,
    pub must_have_coverage: usize,
    pub nice_to_have_coverage: usize,
    pub alternatives: Vec<Alternative>,
}

#[derive(Debug, Serialize)]
pub struct Alternative {
    pub services: Vec<StreamingService>,
    pub total_cost: f64,
    pub nice_to_have_coverage: usize,
}
