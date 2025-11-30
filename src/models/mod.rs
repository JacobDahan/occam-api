use serde::{Deserialize, Serialize};

/// Represents a movie or TV show title
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Title {
    pub id: String,
    pub name: String,
    pub title_type: TitleType,
    pub year: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TitleType {
    Movie,
    Series,
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
