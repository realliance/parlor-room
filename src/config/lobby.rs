//! Lobby configuration

/// Placeholder lobby configuration
#[derive(Debug, Clone)]
pub struct LobbyConfig {
    pub capacity: usize,
    pub wait_time_seconds: u64,
}

impl Default for LobbyConfig {
    fn default() -> Self {
        Self {
            capacity: 4,
            wait_time_seconds: 120,
        }
    }
}
