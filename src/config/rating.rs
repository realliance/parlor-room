//! Rating system configuration

/// Placeholder rating configuration
#[derive(Debug, Clone)]
pub struct RatingConfig {
    pub default_rating: f64,
    pub default_uncertainty: f64,
    pub tolerance: f64,
}

impl Default for RatingConfig {
    fn default() -> Self {
        Self {
            default_rating: 1500.0,
            default_uncertainty: 200.0,
            tolerance: 300.0,
        }
    }
}
