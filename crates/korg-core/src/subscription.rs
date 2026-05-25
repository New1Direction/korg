use serde::{Deserialize, Serialize};

/// The user's registered subscription level.
///
/// Defined in korg-core so that both korg-auth (which stores it in UserSession)
/// and korg-registry (which uses it for access control) can reference the same
/// type without a circular dependency between those two crates.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubscriptionTier {
    Standard,
    Premium,
    Enterprise,
}

impl SubscriptionTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Premium => "Premium",
            Self::Enterprise => "Enterprise",
        }
    }
}
