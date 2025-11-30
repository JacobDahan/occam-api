use axum::{extract::{Query, State}, Extension, Json};
use serde::Deserialize;
use std::sync::Arc;

use crate::{
    error::AppResult,
    middleware::request_id::RequestId,
    models::Title,
    services::title_search::TitleSearcher,
};

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    q: String,
}

/// Handler for title search endpoint
pub async fn search(
    State(searcher): State<Arc<dyn TitleSearcher>>,
    Extension(request_id): Extension<RequestId>,
    Query(params): Query<SearchQuery>,
) -> AppResult<Json<Vec<Title>>> {
    tracing::info!(
        request_id = %request_id,
        query = %params.q,
        "Processing title search request"
    );

    let titles = searcher.search(&params.q).await?;

    tracing::info!(
        request_id = %request_id,
        results_count = titles.len(),
        "Title search completed"
    );

    Ok(Json(titles))
}
