use axum::{extract::{Query, State}, Json};
use serde::Deserialize;
use std::sync::Arc;

use crate::{error::AppResult, models::Title, services::title_search::TitleSearcher};

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    q: String,
}

/// Handler for title search endpoint
pub async fn search(
    State(searcher): State<Arc<dyn TitleSearcher>>,
    Query(params): Query<SearchQuery>,
) -> AppResult<Json<Vec<Title>>> {
    let titles = searcher.search(&params.q).await?;
    Ok(Json(titles))
}
