use axum::{Extension, Json};
use serde::Deserialize;

use crate::{
    error::AppResult, middleware::request_id::RequestId, models::Title, services::recommendations,
};

#[derive(Debug, Deserialize)]
pub struct RecommendationRequest {
    pub user_titles: Vec<String>,
    pub subscribed_services: Vec<String>,
}

/// Handler for recommendations endpoint
pub async fn recommend(
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<RecommendationRequest>,
) -> AppResult<Json<Vec<Title>>> {
    tracing::info!(
        request_id = %request_id,
        user_titles_count = request.user_titles.len(),
        subscribed_services_count = request.subscribed_services.len(),
        "Processing recommendation request"
    );

    let recommendations =
        recommendations::get_recommendations(request.user_titles, request.subscribed_services)
            .await?;

    tracing::info!(
        request_id = %request_id,
        recommendations_count = recommendations.len(),
        "Recommendations completed"
    );

    Ok(Json(recommendations))
}
