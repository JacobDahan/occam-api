use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Represents a streaming service with pricing and available content
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamingService {
    /// Unique identifier for the service
    pub id: Uuid,
    /// Name of the streaming service (e.g., "Netflix", "Hulu")
    pub name: String,
    /// Monthly cost in cents (e.g., 999 = $9.99)
    pub monthly_cost_cents: u32,
    /// List of title IDs available on this service
    pub available_titles: Vec<Uuid>,
}

impl StreamingService {
    /// Creates a new streaming service
    pub fn new(name: String, monthly_cost_cents: u32) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            monthly_cost_cents,
            available_titles: Vec::new(),
        }
    }

    /// Adds a title to the service's available content
    pub fn add_title(&mut self, title_id: Uuid) {
        if !self.available_titles.contains(&title_id) {
            self.available_titles.push(title_id);
        }
    }

    /// Checks if a title is available on this service
    pub fn has_title(&self, title_id: &Uuid) -> bool {
        self.available_titles.contains(title_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_streaming_service() {
        let service = StreamingService::new("Netflix".to_string(), 1599);
        assert_eq!(service.name, "Netflix");
        assert_eq!(service.monthly_cost_cents, 1599);
        assert!(service.available_titles.is_empty());
    }

    #[test]
    fn test_add_title() {
        let mut service = StreamingService::new("Netflix".to_string(), 1599);
        let title_id = Uuid::new_v4();
        service.add_title(title_id);
        assert!(service.has_title(&title_id));
    }

    #[test]
    fn test_add_duplicate_title() {
        let mut service = StreamingService::new("Netflix".to_string(), 1599);
        let title_id = Uuid::new_v4();
        service.add_title(title_id);
        service.add_title(title_id);
        assert_eq!(service.available_titles.len(), 1);
    }
}
