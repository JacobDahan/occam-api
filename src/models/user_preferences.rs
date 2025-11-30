use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::Priority;

/// A title with its priority in user preferences
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TitlePreference {
    /// The ID of the title
    pub title_id: Uuid,
    /// The priority level (must have or nice to have)
    pub priority: Priority,
}

/// User preferences for streaming service optimization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserPreferences {
    /// List of titles with their priorities
    pub titles: Vec<TitlePreference>,
    /// IDs of streaming services the user is currently subscribed to
    pub current_subscriptions: Vec<Uuid>,
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self::new()
    }
}

impl UserPreferences {
    /// Creates empty user preferences
    pub fn new() -> Self {
        Self {
            titles: Vec::new(),
            current_subscriptions: Vec::new(),
        }
    }

    /// Adds a title preference
    pub fn add_title(&mut self, title_id: Uuid, priority: Priority) {
        // Update if exists, otherwise add
        if let Some(existing) = self.titles.iter_mut().find(|t| t.title_id == title_id) {
            existing.priority = priority;
        } else {
            self.titles.push(TitlePreference { title_id, priority });
        }
    }

    /// Adds a current subscription
    pub fn add_subscription(&mut self, service_id: Uuid) {
        if !self.current_subscriptions.contains(&service_id) {
            self.current_subscriptions.push(service_id);
        }
    }

    /// Gets all must-have title IDs
    pub fn must_have_titles(&self) -> Vec<Uuid> {
        self.titles
            .iter()
            .filter(|t| t.priority == Priority::MustHave)
            .map(|t| t.title_id)
            .collect()
    }

    /// Gets all nice-to-have title IDs
    pub fn nice_to_have_titles(&self) -> Vec<Uuid> {
        self.titles
            .iter()
            .filter(|t| t.priority == Priority::NiceToHave)
            .map(|t| t.title_id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_preferences() {
        let prefs = UserPreferences::new();
        assert!(prefs.titles.is_empty());
        assert!(prefs.current_subscriptions.is_empty());
    }

    #[test]
    fn test_add_title() {
        let mut prefs = UserPreferences::new();
        let title_id = Uuid::new_v4();
        prefs.add_title(title_id, Priority::MustHave);
        assert_eq!(prefs.titles.len(), 1);
        assert_eq!(prefs.must_have_titles(), vec![title_id]);
    }

    #[test]
    fn test_update_title_priority() {
        let mut prefs = UserPreferences::new();
        let title_id = Uuid::new_v4();
        prefs.add_title(title_id, Priority::NiceToHave);
        prefs.add_title(title_id, Priority::MustHave);
        assert_eq!(prefs.titles.len(), 1);
        assert_eq!(prefs.must_have_titles(), vec![title_id]);
        assert!(prefs.nice_to_have_titles().is_empty());
    }

    #[test]
    fn test_add_subscription() {
        let mut prefs = UserPreferences::new();
        let service_id = Uuid::new_v4();
        prefs.add_subscription(service_id);
        prefs.add_subscription(service_id); // Duplicate should be ignored
        assert_eq!(prefs.current_subscriptions.len(), 1);
    }
}
