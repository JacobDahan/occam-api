use axum::Json;
use serde::Deserialize;

use crate::{error::AppResult, models::Title, services::recommendations};

#[derive(Debug, Deserialize)]
pub struct RecommendationRequest {
    pub user_titles: Vec<String>,
    pub subscribed_services: Vec<String>,
}

/// Handler for recommendations endpoint
pub async fn recommend(
    Json(request): Json<RecommendationRequest>,
) -> AppResult<Json<Vec<Title>>> {
    let recommendations =
        recommendations::get_recommendations(request.user_titles, request.subscribed_services)
            .await?;
    Ok(Json(recommendations))
}
