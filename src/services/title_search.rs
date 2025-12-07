use crate::{error::AppResult, models::Title, services::providers::StreamingProvider};
use std::sync::Arc;

/// Service function for title search
///
/// Delegates to the configured StreamingProvider, maintaining a clean separation
/// between HTTP routing and business logic.
pub async fn search_titles(
    provider: Arc<dyn StreamingProvider>,
    query: &str,
) -> AppResult<Vec<Title>> {
    provider.search_titles(query).await
}
