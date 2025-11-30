use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Priority level for a title in user preferences
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// Must have - these titles are required in the solution
    MustHave,
    /// Nice to have - these titles are desired but optional
    NiceToHave,
}

/// Represents a movie or TV show title
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Title {
    /// Unique identifier for the title
    pub id: Uuid,
    /// Name of the movie or TV show
    pub name: String,
    /// Type of content (movie or TV show)
    pub content_type: ContentType,
}

/// Type of content
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    Movie,
    TvShow,
}

impl Title {
    /// Creates a new title
    pub fn new(name: String, content_type: ContentType) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            content_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_title() {
        let title = Title::new("The Matrix".to_string(), ContentType::Movie);
        assert_eq!(title.name, "The Matrix");
        assert_eq!(title.content_type, ContentType::Movie);
    }

    #[test]
    fn test_priority_serialization() {
        let must_have = Priority::MustHave;
        let nice_to_have = Priority::NiceToHave;

        let must_have_json = serde_json::to_string(&must_have).unwrap();
        let nice_to_have_json = serde_json::to_string(&nice_to_have).unwrap();

        assert_eq!(must_have_json, "\"must_have\"");
        assert_eq!(nice_to_have_json, "\"nice_to_have\"");
    }
}
