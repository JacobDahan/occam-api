use tracing::instrument;

/// Streaming data provider abstraction
///
/// This module provides a pluggable architecture for different streaming availability
/// data sources (Watchmode, Streaming Availability API, etc.). Each provider implements
/// both title search and availability lookup.
use crate::{
    error::AppResult,
    models::{StreamingAvailability, Title, TitleId},
};

pub mod streaming_availability;
pub mod watchmode;

/// Trait for streaming data providers
///
/// Providers must implement both title search (by name) and availability lookup (by title ID).
/// This ensures consistency: using the same provider for both operations avoids the 2x token
/// cost of converting between different provider ID systems.
#[async_trait::async_trait]
pub trait StreamingProvider: Send + Sync {
    /// Search for titles by name
    ///
    /// Returns a list of matching titles with IDs for downstream availability lookups.
    async fn search_titles(&self, query: &str) -> AppResult<Vec<Title>>;

    /// Fetch streaming availability by title ID
    ///
    /// Accepts either IMDB ID or provider-specific ID. Provider-specific IDs may be more
    /// efficient (e.g., Watchmode charges less for native ID lookups vs IMDB ID lookups).
    ///
    /// Returns availability data including which services have the title and pricing.
    async fn fetch_availability(&self, title_id: &TitleId) -> AppResult<StreamingAvailability>;

    /// Fetch availability for multiple titles in parallel
    ///
    /// Default implementation calls fetch_availability for each ID in parallel.
    /// Providers can override for bulk API endpoints if available.
    async fn fetch_availability_batch(
        &self,
        title_ids: Vec<TitleId>,
    ) -> AppResult<Vec<StreamingAvailability>> {
        let mut tasks = Vec::new();

        for title_id in title_ids {
            let provider = self.clone_for_task();
            let task = tokio::spawn(async move { provider.fetch_availability(&title_id).await });
            tasks.push(task);
        }

        let mut results = Vec::new();
        let mut errors = Vec::new();

        for task in tasks {
            match task.await {
                Ok(Ok(availability)) => results.push(availability),
                Ok(Err(e)) => {
                    tracing::error!(error = %e, "Availability fetch failed for title");
                    errors.push(e);
                }
                Err(e) => {
                    tracing::error!(error = %e, "Task join error");
                    errors.push(crate::error::AppError::Internal(e.to_string()));
                }
            }
        }

        if !errors.is_empty() {
            tracing::warn!(
                success_count = results.len(),
                error_count = errors.len(),
                "Partial availability fetch failure"
            );
        }

        if results.is_empty() && !errors.is_empty() {
            return Err(crate::error::AppError::ExternalApi(
                "Failed to fetch any availability data".to_string(),
            ));
        }

        Ok(results)
    }

    /// Clone provider for parallel task execution
    ///
    /// Required because providers need to be moved into tokio tasks.
    fn clone_for_task(&self) -> Box<dyn StreamingProvider>;

    /// Provider name for logging and debugging
    fn name(&self) -> &'static str;
}
