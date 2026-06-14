//! Campaign — State Machine and .ktrans Lifecycle
//!
//! Encapsulates the campaign lifecycle: phases, signing, and ktrans persistence.
//! The `LeaderOrchestrator` remains a thin coordinator that calls into this module
//! rather than owning the signing and serialization logic itself.
//!
//! # Campaign Phase State Machine
//!
//! ```text
//! Initializing → Planning → Contracting → Dispatching
//!                                            ↓
//!                                       Evaluating → Committing → Complete
//!                                                        ↓
//!                                                    Aborted (on doom-loop)
//! ```

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// =========================================================================
// Campaign Phase State Machine
// =========================================================================

/// Ordered states of a campaign run. Transitions are linear except for
/// `Aborted` which can be reached from any phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CampaignPhase {
    Initializing,
    Planning,
    Contracting,
    Dispatching,
    Evaluating,
    Committing,
    Complete,
    Aborted,
}

impl CampaignPhase {
    /// Returns the next valid phase in the linear happy path.
    pub fn next(&self) -> Option<CampaignPhase> {
        match self {
            CampaignPhase::Initializing => Some(CampaignPhase::Planning),
            CampaignPhase::Planning => Some(CampaignPhase::Contracting),
            CampaignPhase::Contracting => Some(CampaignPhase::Dispatching),
            CampaignPhase::Dispatching => Some(CampaignPhase::Evaluating),
            CampaignPhase::Evaluating => Some(CampaignPhase::Committing),
            CampaignPhase::Committing => Some(CampaignPhase::Complete),
            CampaignPhase::Complete => None,
            CampaignPhase::Aborted => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            CampaignPhase::Initializing => "initializing",
            CampaignPhase::Planning => "planning",
            CampaignPhase::Contracting => "contracting",
            CampaignPhase::Dispatching => "dispatching",
            CampaignPhase::Evaluating => "evaluating",
            CampaignPhase::Committing => "committing",
            CampaignPhase::Complete => "complete",
            CampaignPhase::Aborted => "aborted",
        }
    }
}

// =========================================================================
// Campaign Context
// =========================================================================

/// Immutable configuration for a campaign run.
#[derive(Debug, Clone)]
pub struct CampaignConfig {
    pub session_id: Uuid,
    pub root_task: String,
    pub max_rounds: usize,
    pub goal_mode: bool,
}

/// Mutable tip state for the campaign's Merkle-DAG ledger.
/// Each committed .ktrans appends its hash to this tip set.
#[derive(Debug, Clone, Default)]
pub struct CampaignTips {
    pub hashes: Vec<String>,
}

impl CampaignTips {
    pub fn update(&mut self, new_hash: String) {
        self.hashes = vec![new_hash];
    }

    pub fn current(&self) -> Vec<String> {
        self.hashes.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_campaign_phase_transitions_happy_path() {
        let mut phase = CampaignPhase::Initializing;
        let expected = [
            CampaignPhase::Planning,
            CampaignPhase::Contracting,
            CampaignPhase::Dispatching,
            CampaignPhase::Evaluating,
            CampaignPhase::Committing,
            CampaignPhase::Complete,
        ];
        for &next_expected in &expected {
            phase = phase.next().expect("transition should exist");
            assert_eq!(phase, next_expected);
        }
        assert!(phase.next().is_none(), "Complete has no successor");
    }

    #[test]
    fn test_campaign_phase_aborted_terminal() {
        assert!(CampaignPhase::Aborted.next().is_none());
    }

    #[test]
    fn test_campaign_tips_update() {
        let mut tips = CampaignTips::default();
        assert!(tips.current().is_empty());
        tips.update("abc123".to_string());
        assert_eq!(tips.current(), vec!["abc123".to_string()]);
        tips.update("def456".to_string());
        // Only the latest tip is kept (linear chain)
        assert_eq!(tips.current(), vec!["def456".to_string()]);
    }

    #[test]
    fn test_campaign_phase_as_str() {
        assert_eq!(CampaignPhase::Initializing.as_str(), "initializing");
        assert_eq!(CampaignPhase::Aborted.as_str(), "aborted");
        assert_eq!(CampaignPhase::Complete.as_str(), "complete");
    }
}
