use axum::Json;

use crate::{
    error::AppResult,
    models::{OptimizationRequest, OptimizationResponse},
    services::optimization,
};

/// Handler for optimization endpoint
pub async fn optimize(
    Json(request): Json<OptimizationRequest>,
) -> AppResult<Json<OptimizationResponse>> {
    let response = optimization::optimize_services(request).await?;
    Ok(Json(response))
}
