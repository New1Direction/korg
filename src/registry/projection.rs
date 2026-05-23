use serde::Serialize;
use uuid::Uuid;
use std::collections::HashMap;
use super::log::JournalEvent;
use super::log::CapabilityEvent;
use super::log::HlcTimestamp;
use super::plan::TransitionState;

pub trait Projection: Send + Sync {
    type State: Serialize + Clone;

    /// The unique identifier of the projection
    fn name(&self) -> &'static str;

    /// The operational version of this projection.
    fn projection_version(&self) -> u32;

    /// Apply a single log event, folding it into the current state.
    /// MUST be a pure, side-effect-free function.
    fn apply(&mut self, event: &JournalEvent) -> Result<(), String>;

    /// Extract a point-in-time serializable snapshot of the projected state.
    fn snapshot(&self) -> Self::State;
    
    /// Reset the projection to its default initial state
    fn reset(&mut self);

    /// Rebuild the projection state chronologically from a history of log events.
    fn rebuild(&mut self, events: &[JournalEvent]) -> Result<(), String> {
        self.reset();
        for event in events {
            self.apply(event)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum CampaignStatus {
    Idle,
    Active,
    Success,
    Failed,
    Aborted,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct FailureSummary {
    pub step_target: String,
    pub effect_id: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct CampaignState {
    pub campaign_id: Option<Uuid>,
    pub status: CampaignStatus,
    pub current_phase: String,
    pub active_workers: usize,
    pub retries: u32,
    pub failures: Vec<FailureSummary>,
    pub started_at_seq: u64,
    pub latest_clock: HlcTimestamp,
}

pub struct CampaignProjection {
    state: CampaignState,
}

impl CampaignProjection {
    pub fn new() -> Self {
        Self {
            state: CampaignState {
                campaign_id: None,
                status: CampaignStatus::Idle,
                current_phase: "Idle".to_string(),
                active_workers: 0,
                retries: 0,
                failures: vec![],
                started_at_seq: 0,
                latest_clock: HlcTimestamp::default(),
            }
        }
    }
}

impl Projection for CampaignProjection {
    type State = CampaignState;

    fn name(&self) -> &'static str {
        "campaign_projection"
    }

    fn projection_version(&self) -> u32 {
        1
    }

    fn reset(&mut self) {
        *self = Self::new();
    }

    fn snapshot(&self) -> Self::State {
        self.state.clone()
    }

    fn apply(&mut self, envelope: &JournalEvent) -> Result<(), String> {
        self.state.latest_clock = envelope.metadata.emitted_at;

        match &envelope.event {
            CapabilityEvent::LeaseAcquired { owner_id, .. } => {
                if self.state.status == CampaignStatus::Idle || self.state.status == CampaignStatus::Aborted {
                    self.state.campaign_id = Some(*owner_id);
                    self.state.status = CampaignStatus::Active;
                    self.state.started_at_seq = envelope.seq_id;
                    self.state.current_phase = "Acquired Leases — Initializing".to_string();
                }
            }
            CapabilityEvent::TransitionStateChanged { state, .. } => {
                self.state.current_phase = format!("Transitioning: {:?}", state);
                match state {
                    TransitionState::Applied => {
                        self.state.status = CampaignStatus::Success;
                        self.state.current_phase = "Success".to_string();
                    }
                    TransitionState::Failed | TransitionState::RolledBack => {
                        self.state.status = CampaignStatus::Failed;
                        self.state.current_phase = "Failed".to_string();
                    }
                    _ => {}
                }
            }
            CapabilityEvent::EffectStarted { step_target, effect_id, .. } => {
                self.state.current_phase = format!("Executing step: {} (Effect {})", step_target, effect_id);
                self.state.active_workers = self.state.active_workers.max(1);
            }
            CapabilityEvent::EffectRetrying { .. } => {
                self.state.retries += 1;
            }
            CapabilityEvent::EffectFailed { step_target, effect_id, reason, .. } => {
                self.state.failures.push(FailureSummary {
                    step_target: step_target.clone(),
                    effect_id: *effect_id,
                    reason: reason.clone(),
                });
            }
            CapabilityEvent::LeaseReleased { .. } => {
                if self.state.status == CampaignStatus::Active {
                    self.state.status = CampaignStatus::Aborted;
                    self.state.current_phase = "Aborted".to_string();
                }
            }
            _ => {}
        }
        Ok(())
    }
}

pub trait DynamicProjection: Send + Sync {
    fn name(&self) -> &'static str;
    fn projection_version(&self) -> u32;
    fn apply_dynamic(&mut self, event: &JournalEvent) -> Result<(), String>;
    fn snapshot_json(&self) -> Result<serde_json::Value, String>;
    fn reset_dynamic(&mut self);
    fn rebuild_dynamic(&mut self, events: &[JournalEvent]) -> Result<(), String>;
}

impl<T> DynamicProjection for T 
where 
    T: Projection,
    T::State: 'static
{
    fn name(&self) -> &'static str {
        self.name()
    }
    fn projection_version(&self) -> u32 {
        self.projection_version()
    }
    fn apply_dynamic(&mut self, event: &JournalEvent) -> Result<(), String> {
        self.apply(event)
    }
    fn snapshot_json(&self) -> Result<serde_json::Value, String> {
        serde_json::to_value(&self.snapshot()).map_err(|e| e.to_string())
    }
    fn reset_dynamic(&mut self) {
        self.reset();
    }
    fn rebuild_dynamic(&mut self, events: &[JournalEvent]) -> Result<(), String> {
        self.rebuild(events)
    }
}

pub struct ProjectionEngine {
    pub projections: HashMap<String, Box<dyn DynamicProjection>>,
}

impl ProjectionEngine {
    pub fn new() -> Self {
        let mut engine = Self {
            projections: HashMap::new(),
        };
        // Register default campaign projection
        engine.register(CampaignProjection::new());
        engine
    }

    pub fn register<T>(&mut self, projection: T)
    where
        T: Projection + 'static,
        T::State: 'static
    {
        let name = projection.name().to_string();
        self.projections.insert(name, Box::new(projection));
    }

    pub fn apply(&mut self, event: &JournalEvent) -> Result<(), String> {
        for proj in self.projections.values_mut() {
            proj.apply_dynamic(event)?;
        }
        Ok(())
    }

    pub fn get_projection_state(&self, name: &str) -> Option<serde_json::Value> {
        self.projections.get(name).and_then(|p| p.snapshot_json().ok())
    }

    pub fn get_projection_version(&self, name: &str) -> Option<u32> {
        self.projections.get(name).map(|p| p.projection_version())
    }

    pub fn reset_all(&mut self) {
        for proj in self.projections.values_mut() {
            proj.reset_dynamic();
        }
    }

    pub fn rebuild_projection(&mut self, name: &str, events: &[JournalEvent]) -> Result<(), String> {
        if let Some(proj) = self.projections.get_mut(name) {
            proj.rebuild_dynamic(events)?;
            Ok(())
        } else {
            Err(format!("Projection '{}' not registered in engine", name))
        }
    }

    pub fn rebuild_all(&mut self, events: &[JournalEvent]) -> Result<(), String> {
        for proj in self.projections.values_mut() {
            proj.rebuild_dynamic(events)?;
        }
        Ok(())
    }
}
