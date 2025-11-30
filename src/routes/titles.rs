use axum::{extract::Query, Json};
use serde::Deserialize;

use crate::{error::AppResult, models::Title, services::title_search};

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    q: String,
}

/// Handler for title search endpoint
pub async fn search(Query(params): Query<SearchQuery>) -> AppResult<Json<Vec<Title>>> {
    let titles = title_search::search_titles(&params.q).await?;
    Ok(Json(titles))
}
