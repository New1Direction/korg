use crate::log::CapabilityEvent;
use crate::log::HlcTimestamp;
use crate::log::JournalEvent;
use crate::plan::TransitionState;
use serde::Serialize;
use std::collections::HashMap;
use uuid::Uuid;

pub(crate) trait Projection: Send + Sync {
    type State: Serialize + Clone;

    fn name(&self) -> &'static str;
    fn projection_version(&self) -> u32;

    /// Apply a single log event, folding it into the current state.
    /// MUST be a pure, side-effect-free function.
    fn apply(&mut self, event: &JournalEvent) -> Result<(), String>;

    fn snapshot(&self) -> Self::State;
    fn reset(&mut self);

    fn rebuild(&mut self, events: &[JournalEvent]) -> Result<(), String> {
        self.reset();
        for event in events {
            self.apply(event)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, serde::Deserialize, PartialEq, Eq)]
pub(crate) enum CampaignStatus {
    Idle,
    Active,
    Success,
    Failed,
    Aborted,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub(crate) struct FailureSummary {
    pub(crate) step_target: String,
    pub(crate) effect_id: usize,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub(crate) struct CampaignState {
    pub(crate) campaign_id: Option<Uuid>,
    pub(crate) status: CampaignStatus,
    pub(crate) current_phase: String,
    pub(crate) active_workers: usize,
    pub(crate) retries: u32,
    pub(crate) failures: Vec<FailureSummary>,
    pub(crate) started_at_seq: u64,
    pub(crate) latest_clock: HlcTimestamp,
}

pub(crate) struct CampaignProjection {
    state: CampaignState,
}

impl CampaignProjection {
    pub(crate) fn new() -> Self {
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
            },
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
                if self.state.status == CampaignStatus::Idle
                    || self.state.status == CampaignStatus::Aborted
                {
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
            CapabilityEvent::EffectStarted {
                step_target,
                effect_id,
                ..
            } => {
                self.state.current_phase =
                    format!("Executing step: {} (Effect {})", step_target, effect_id);
                self.state.active_workers = self.state.active_workers.max(1);
            }
            CapabilityEvent::EffectRetrying { .. } => {
                self.state.retries += 1;
            }
            CapabilityEvent::EffectFailed {
                step_target,
                effect_id,
                reason,
                ..
            } => {
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

pub(crate) trait DynamicProjection: Send + Sync {
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
    T::State: 'static,
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
    pub(crate) projections: HashMap<String, Box<dyn DynamicProjection>>,
}

impl ProjectionEngine {
    pub fn new() -> Self {
        let mut engine = Self {
            projections: HashMap::new(),
        };
        engine.register(CampaignProjection::new());
        engine
    }

    pub(crate) fn register<T>(&mut self, projection: T)
    where
        T: Projection + 'static,
        T::State: 'static,
    {
        let name = projection.name().to_string();
        self.projections.insert(name, Box::new(projection));
    }

    pub(crate) fn apply(&mut self, event: &JournalEvent) -> Result<(), String> {
        for proj in self.projections.values_mut() {
            proj.apply_dynamic(event)?;
        }
        Ok(())
    }

    pub(crate) fn get_projection_state(&self, name: &str) -> Option<serde_json::Value> {
        self.projections
            .get(name)
            .and_then(|p| p.snapshot_json().ok())
    }

    pub(crate) fn get_projection_version(&self, name: &str) -> Option<u32> {
        self.projections.get(name).map(|p| p.projection_version())
    }

    pub(crate) fn reset_all(&mut self) {
        for proj in self.projections.values_mut() {
            proj.reset_dynamic();
        }
    }

    pub(crate) fn rebuild_projection(
        &mut self,
        name: &str,
        events: &[JournalEvent],
    ) -> Result<(), String> {
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
