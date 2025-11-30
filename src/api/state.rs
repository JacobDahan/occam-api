use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use uuid::Uuid;

use crate::models::{StreamingService, Title, UserPreferences};

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub inner: Arc<RwLock<AppStateInner>>,
}

/// Inner state that can be modified
pub struct AppStateInner {
    pub services: HashMap<Uuid, StreamingService>,
    pub titles: HashMap<Uuid, Title>,
    pub user_preferences: UserPreferences,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    /// Creates a new empty application state
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(AppStateInner {
                services: HashMap::new(),
                titles: HashMap::new(),
                user_preferences: UserPreferences::new(),
            })),
        }
    }
}
