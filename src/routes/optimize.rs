use axum::{Extension, Json};

use crate::{
    error::AppResult,
    middleware::request_id::RequestId,
    models::{OptimizationRequest, OptimizationResponse},
    services::optimization,
};

/// Handler for optimization endpoint
pub async fn optimize(
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<OptimizationRequest>,
) -> AppResult<Json<OptimizationResponse>> {
    tracing::info!(
        request_id = %request_id,
        must_have_count = request.must_have.len(),
        nice_to_have_count = request.nice_to_have.len(),
        "Processing optimization request"
    );

    let response = optimization::optimize_services(request).await?;

    tracing::info!(
        request_id = %request_id,
        "Optimization completed"
    );

    Ok(Json(response))
}
