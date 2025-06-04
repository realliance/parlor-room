//! Rating system configuration

use serde::{Deserialize, Serialize};

/// Comprehensive rating system configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingConfig {
    /// Default rating for new players
    pub default_rating: f64,
    /// Default uncertainty for new players
    pub default_uncertainty: f64,
    /// Maximum rating difference for matching
    pub tolerance: f64,
    /// Rating system type ("weng_lin", "elo", "trueskill")
    pub rating_system: String,
    /// Uncertainty decay rate per game
    pub uncertainty_decay: f64,
    /// Minimum uncertainty value
    pub min_uncertainty: f64,
    /// Maximum uncertainty value
    pub max_uncertainty: f64,
    /// Beta parameter for Weng-Lin system
    pub beta: f64,
    /// Rating convergence factor
    pub convergence_tolerance: f64,
    /// Whether to enable dynamic rating adjustments
    pub enable_dynamic_adjustments: bool,
    /// Provisional games count (games before rating stabilizes)
    pub provisional_games: u32,
    /// Rating floor (minimum rating)
    pub rating_floor: f64,
    /// Rating ceiling (maximum rating)
    pub rating_ceiling: f64,
}

impl Default for RatingConfig {
    fn default() -> Self {
        Self {
            default_rating: 1500.0,
            default_uncertainty: 200.0,
            tolerance: 300.0,
            rating_system: "weng_lin".to_string(),
            uncertainty_decay: 0.95,
            min_uncertainty: 50.0,
            max_uncertainty: 350.0,
            beta: 200.0,
            convergence_tolerance: 0.0001,
            enable_dynamic_adjustments: true,
            provisional_games: 10,
            rating_floor: 100.0,
            rating_ceiling: 3000.0,
        }
    }
}

impl RatingConfig {
    /// Create configuration for conservative rating changes
    pub fn conservative() -> Self {
        Self {
            uncertainty_decay: 0.98, // Slower uncertainty reduction
            beta: 250.0,             // Higher beta for smaller rating changes
            tolerance: 200.0,        // Stricter matching
            provisional_games: 15,   // More games before stabilization
            ..Default::default()
        }
    }

    /// Create configuration for aggressive rating changes
    pub fn aggressive() -> Self {
        Self {
            uncertainty_decay: 0.90, // Faster uncertainty reduction
            beta: 150.0,             // Lower beta for larger rating changes
            tolerance: 400.0,        // Looser matching
            provisional_games: 5,    // Fewer games before stabilization
            ..Default::default()
        }
    }

    /// Create configuration for beginner-friendly ratings
    pub fn beginner_friendly() -> Self {
        Self {
            default_rating: 1200.0,     // Lower starting rating
            default_uncertainty: 300.0, // Higher initial uncertainty
            tolerance: 500.0,           // Very loose matching
            provisional_games: 20,      // More provisional games
            rating_floor: 800.0,        // Higher floor to prevent extreme drops
            ..Default::default()
        }
    }

    /// Create configuration for competitive play
    pub fn competitive() -> Self {
        Self {
            default_rating: 1500.0,
            default_uncertainty: 150.0, // Lower initial uncertainty
            tolerance: 150.0,           // Strict matching
            provisional_games: 25,      // Many provisional games
            uncertainty_decay: 0.97,    // Slow uncertainty reduction
            beta: 200.0,
            ..Default::default()
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.default_rating < self.rating_floor || self.default_rating > self.rating_ceiling {
            return Err("Default rating must be within rating floor and ceiling".to_string());
        }

        if self.default_uncertainty < self.min_uncertainty
            || self.default_uncertainty > self.max_uncertainty
        {
            return Err(
                "Default uncertainty must be within min and max uncertainty bounds".to_string(),
            );
        }

        if self.min_uncertainty >= self.max_uncertainty {
            return Err("Minimum uncertainty must be less than maximum uncertainty".to_string());
        }

        if self.rating_floor >= self.rating_ceiling {
            return Err("Rating floor must be less than rating ceiling".to_string());
        }

        if self.uncertainty_decay <= 0.0 || self.uncertainty_decay > 1.0 {
            return Err("Uncertainty decay must be between 0.0 and 1.0".to_string());
        }

        if self.beta <= 0.0 {
            return Err("Beta must be positive".to_string());
        }

        if self.tolerance < 0.0 {
            return Err("Tolerance must be non-negative".to_string());
        }

        if self.convergence_tolerance <= 0.0 {
            return Err("Convergence tolerance must be positive".to_string());
        }

        Ok(())
    }

    /// Get configuration suitable for the Weng-Lin rating system
    pub fn to_weng_lin_config(&self) -> crate::rating::weng_lin::ExtendedWengLinConfig {
        crate::rating::weng_lin::ExtendedWengLinConfig {
            weng_lin_config: skillratings::weng_lin::WengLinConfig {
                beta: self.beta,
                uncertainty_tolerance: self.convergence_tolerance,
            },
            initial_rating: self.default_rating,
            initial_uncertainty: self.default_uncertainty,
        }
    }
}
