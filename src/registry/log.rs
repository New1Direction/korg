use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

pub static IS_PREVIEW_MODE: AtomicBool = AtomicBool::new(false);

use super::plan::TransitionState;
use super::types::CapabilityState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "event_type")]
pub enum CapabilityEvent {
    CapabilityEnabled {
        plan_id: Uuid,
        id: String,
        timestamp: DateTime<Utc>,
    },
    CapabilityDisabled {
        plan_id: Uuid,
        id: String,
        timestamp: DateTime<Utc>,
    },
    CapabilityScaled {
        plan_id: Uuid,
        id: String,
        scale: f32,
        timestamp: DateTime<Utc>,
    },
    CapabilityConflictDetected {
        plan_id: Uuid,
        id: String,
        conflicting_with: String,
        timestamp: DateTime<Utc>,
    },
    TransitionStateChanged {
        plan_id: Uuid,
        state: TransitionState,
        timestamp: DateTime<Utc>,
    },

    // Live granular progress telemetry events
    EffectStarted {
        plan_id: Uuid,
        step_target: String,
        effect_id: usize,
        timestamp: DateTime<Utc>,
    },
    EffectCompleted {
        plan_id: Uuid,
        step_target: String,
        effect_id: usize,
        timestamp: DateTime<Utc>,
    },
    EffectFailed {
        plan_id: Uuid,
        step_target: String,
        effect_id: usize,
        reason: String,
        timestamp: DateTime<Utc>,
    },
    EffectRetrying {
        plan_id: Uuid,
        step_target: String,
        effect_id: usize,
        retry_count: usize,
        timestamp: DateTime<Utc>,
    },

    TransitionRolledBack {
        plan_id: Uuid,
        target_id: String,
        reason: String,
        timestamp: DateTime<Utc>,
    },
    LeaseAcquired {
        id: String,
        owner_id: Uuid,
        duration_secs: u64,
        timestamp: DateTime<Utc>,
    },
    LeaseReleased {
        id: String,
        owner_id: Uuid,
        timestamp: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EventTier {
    Governance,
    Effect,
    Projection,
    Telemetry,
}

impl CapabilityEvent {
    pub fn campaign_id(&self) -> Uuid {
        match self {
            CapabilityEvent::CapabilityEnabled { plan_id, .. } => *plan_id,
            CapabilityEvent::CapabilityDisabled { plan_id, .. } => *plan_id,
            CapabilityEvent::CapabilityScaled { plan_id, .. } => *plan_id,
            CapabilityEvent::CapabilityConflictDetected { plan_id, .. } => *plan_id,
            CapabilityEvent::TransitionStateChanged { plan_id, .. } => *plan_id,
            CapabilityEvent::EffectStarted { plan_id, .. } => *plan_id,
            CapabilityEvent::EffectCompleted { plan_id, .. } => *plan_id,
            CapabilityEvent::EffectFailed { plan_id, .. } => *plan_id,
            CapabilityEvent::EffectRetrying { plan_id, .. } => *plan_id,
            CapabilityEvent::TransitionRolledBack { plan_id, .. } => *plan_id,
            CapabilityEvent::LeaseAcquired { owner_id, .. } => *owner_id,
            CapabilityEvent::LeaseReleased { owner_id, .. } => *owner_id,
        }
    }

    pub fn tier(&self) -> EventTier {
        match self {
            CapabilityEvent::CapabilityEnabled { .. }
            | CapabilityEvent::CapabilityDisabled { .. }
            | CapabilityEvent::CapabilityScaled { .. }
            | CapabilityEvent::CapabilityConflictDetected { .. }
            | CapabilityEvent::TransitionStateChanged { .. }
            | CapabilityEvent::TransitionRolledBack { .. }
            | CapabilityEvent::LeaseAcquired { .. }
            | CapabilityEvent::LeaseReleased { .. } => EventTier::Governance,

            CapabilityEvent::EffectStarted { .. }
            | CapabilityEvent::EffectCompleted { .. }
            | CapabilityEvent::EffectFailed { .. }
            | CapabilityEvent::EffectRetrying { .. } => EventTier::Effect,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct HlcTimestamp {
    pub physical: i64,
    pub logical: u32,
    pub actor_id: u32,
}

impl Default for HlcTimestamp {
    fn default() -> Self {
        Self {
            physical: 0,
            logical: 0,
            actor_id: 1,
        }
    }
}

impl HlcTimestamp {
    pub fn new(physical: i64, logical: u32, actor_id: u32) -> Self {
        Self {
            physical,
            logical,
            actor_id,
        }
    }

    /// Pure function for local clock ticks
    pub fn tick(&self, wall_clock: i64) -> Self {
        let new_physical = std::cmp::max(wall_clock, self.physical);
        let new_logical = if new_physical == self.physical {
            self.logical + 1
        } else {
            0
        };
        Self {
            physical: new_physical,
            logical: new_logical,
            actor_id: self.actor_id,
        }
    }

    /// Pure function for merging external clock states
    pub fn merge(&self, external: &HlcTimestamp, wall_clock: i64) -> Self {
        if external.physical - wall_clock > 500 {
            tracing::warn!(
                "HLC clock drift limit exceeded! External physical: {}, Local wall clock: {}",
                external.physical,
                wall_clock
            );
        }

        let new_physical =
            std::cmp::max(wall_clock, std::cmp::max(self.physical, external.physical));
        let new_logical = if new_physical == self.physical && new_physical == external.physical {
            std::cmp::max(self.logical, external.logical) + 1
        } else if new_physical == self.physical {
            self.logical + 1
        } else if new_physical == external.physical {
            external.logical + 1
        } else {
            0
        };

        Self {
            physical: new_physical,
            logical: new_logical,
            actor_id: self.actor_id,
        }
    }
}

impl From<u64> for HlcTimestamp {
    fn from(val: u64) -> Self {
        Self {
            physical: 0,
            logical: val as u32,
            actor_id: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventMetadata {
    pub event_id: Uuid,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
    pub root_event_id: Uuid,

    pub actor_id: String,
    pub campaign_id: Uuid,

    pub emitted_at: HlcTimestamp,

    pub branch_id: Option<Uuid>,
    pub speculative: bool,

    pub retry_count: u32,

    pub tier: EventTier,
    pub span_id: Option<Uuid>,

    pub tags: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JournalEvent {
    pub seq_id: u64,
    pub metadata: EventMetadata,
    pub event: CapabilityEvent,
}

/// Periodic state checkpoint to bypass full event replay cold-starts
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapabilitySnapshot {
    pub checkpoint_id: Uuid,
    pub plan_id: Uuid,
    pub active_states: HashMap<String, CapabilityState>,
    pub timestamp: DateTime<Utc>,
}

pub struct CapabilityJournal {
    pub events: Vec<JournalEvent>,
    pub snapshots: Vec<CapabilitySnapshot>,
    pub journal_path: PathBuf,
    pub snapshot_path: PathBuf,
    pub snapshot_interval: usize,
    pub lock_path: PathBuf,
    pub last_seq_id: u64,
    pub clock: HlcTimestamp,
    pub is_speculative: bool, // In-memory speculative execution flag
}

impl CapabilityJournal {
    pub fn new(
        journal_path: PathBuf,
        snapshot_path: PathBuf,
        snapshot_interval: usize,
        lock_path: PathBuf,
    ) -> Self {
        let mut journal = Self {
            events: vec![],
            snapshots: vec![],
            journal_path,
            snapshot_path,
            snapshot_interval,
            lock_path,
            last_seq_id: 0,
            clock: HlcTimestamp::default(),
            is_speculative: IS_PREVIEW_MODE.load(Ordering::Relaxed),
        };
        let _ = journal.load();
        journal
    }

    pub fn default_journal() -> Self {
        let root = crate::paths::project_root().join(".korg");
        let _ = std::fs::create_dir_all(&root);
        Self::new(
            root.join("capability_journal.json"),
            root.join("capability_snapshots.json"),
            10,
            root.join("capability_journal.lock"),
        )
    }

    /// Load event history and snapshots with shared lock
    pub fn load(&mut self) -> Result<(), String> {
        let lock_file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&self.lock_path)
            .map_err(|e| e.to_string())?;

        if lock_file.lock_shared().is_ok() {
            if self.journal_path.exists() {
                if let Ok(mut f) = std::fs::File::open(&self.journal_path) {
                    let mut content = String::new();
                    if f.read_to_string(&mut content).is_ok() {
                        if let Ok(parsed) = serde_json::from_str(&content) {
                            self.events = parsed;
                        }
                    }
                }
            }
            if self.snapshot_path.exists() {
                if let Ok(mut f) = std::fs::File::open(&self.snapshot_path) {
                    let mut content = String::new();
                    if f.read_to_string(&mut content).is_ok() {
                        if let Ok(parsed) = serde_json::from_str(&content) {
                            self.snapshots = parsed;
                        }
                    }
                }
            }

            // Reconstruct monotonic sequence ID and HLC clock from loaded log history
            self.last_seq_id = self.events.iter().map(|e| e.seq_id).max().unwrap_or(0);
            self.clock = self
                .events
                .iter()
                .map(|e| e.metadata.emitted_at)
                .max()
                .unwrap_or_default();

            let _ = lock_file.unlock();
        }
        Ok(())
    }

    /// Append event with full custom metadata control
    pub fn append_with_metadata(&mut self, event: CapabilityEvent, metadata: EventMetadata) {
        self.last_seq_id += 1;
        // Merge clock to maintain local HLC monotonicity
        let wall_clock = chrono::Utc::now().timestamp_millis();
        self.clock = self.clock.merge(&metadata.emitted_at, wall_clock);

        let journal_event = JournalEvent {
            seq_id: self.last_seq_id,
            metadata,
            event,
        };
        self.events.push(journal_event);
        let _ = self.flush();
    }

    /// Append event and flush with exclusive lock, automatically generating metadata
    pub fn append(&mut self, event: CapabilityEvent) {
        let campaign_id = event.campaign_id();
        let parent = self.events.last();
        let event_id = Uuid::new_v4();

        let causation_id = parent.map(|e| e.metadata.event_id);
        let root_event_id = parent.map(|e| e.metadata.root_event_id).unwrap_or(event_id);
        let tier = event.tier();

        let retry_count = match &event {
            CapabilityEvent::EffectRetrying { retry_count, .. } => *retry_count as u32,
            _ => 0,
        };

        let wall_clock = chrono::Utc::now().timestamp_millis();
        let emitted_at = self.clock.tick(wall_clock);

        let metadata = EventMetadata {
            event_id,
            correlation_id: campaign_id,
            causation_id,
            root_event_id,
            actor_id: "coordinator".to_string(),
            campaign_id,
            emitted_at,
            branch_id: None,
            speculative: self.is_speculative,
            retry_count,
            tier,
            span_id: None,
            tags: BTreeMap::new(),
        };

        self.last_seq_id += 1;
        self.clock = emitted_at;
        let journal_event = JournalEvent {
            seq_id: self.last_seq_id,
            metadata,
            event,
        };
        self.events.push(journal_event);
        let _ = self.flush();
    }

    /// Synchronize the causal logical clock with external causal clocks
    pub fn synchronize_clock(&mut self, external_clock: HlcTimestamp) {
        let wall_clock = chrono::Utc::now().timestamp_millis();
        self.clock = self.clock.merge(&external_clock, wall_clock);
    }

    /// Insert snapshot checkpoint
    pub fn save_snapshot(
        &mut self,
        plan_id: Uuid,
        active_states: HashMap<String, CapabilityState>,
    ) {
        let snapshot = CapabilitySnapshot {
            checkpoint_id: Uuid::new_v4(),
            plan_id,
            active_states,
            timestamp: Utc::now(),
        };
        self.snapshots.push(snapshot);
        let _ = self.flush();
    }

    /// Rewind the event journal to a specific target sequence ID, truncating all subsequent events
    pub fn rewind(&mut self, target_seq_id: u64) -> Result<(), String> {
        if target_seq_id > self.last_seq_id {
            return Err(format!("Cannot rewind to sequence ID {} which is greater than the current last sequence ID {}", target_seq_id, self.last_seq_id));
        }

        self.events.retain(|e| e.seq_id <= target_seq_id);
        self.last_seq_id = target_seq_id;
        self.clock = self
            .events
            .iter()
            .map(|e| e.metadata.emitted_at)
            .max()
            .unwrap_or_default();

        self.flush()?;
        Ok(())
    }

    /// Flush history to disk atomically with exclusive advisory locking
    pub fn flush(&self) -> Result<(), String> {
        if self.is_speculative {
            return Ok(()); // Bypasses all disk writes completely during dry-run speculative preview mode
        }
        let lock_file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&self.lock_path)
            .map_err(|e| e.to_string())?;

        if lock_file.lock_exclusive().is_ok() {
            // Write events atomically
            let tmp_journal = self.journal_path.with_extension("tmp");
            if let Ok(serialized) = serde_json::to_string_pretty(&self.events) {
                if let Ok(mut f) = std::fs::File::create(&tmp_journal) {
                    let _ = f.write_all(serialized.as_bytes());
                    let _ = f.sync_all();
                    let _ = std::fs::rename(&tmp_journal, &self.journal_path);
                }
            }

            // Write snapshots atomically
            let tmp_snapshot = self.snapshot_path.with_extension("tmp");
            if let Ok(serialized) = serde_json::to_string_pretty(&self.snapshots) {
                if let Ok(mut f) = std::fs::File::create(&tmp_snapshot) {
                    let _ = f.write_all(serialized.as_bytes());
                    let _ = f.sync_all();
                    let _ = std::fs::rename(&tmp_snapshot, &self.snapshot_path);
                }
            }

            let _ = lock_file.unlock();
        }
        Ok(())
    }

    /// Return the last `n` events as a JSONL string (one JSON object per line).
    /// Suitable for streaming to `GET /api/journal`.
    pub fn to_json_lines(&self, n: usize) -> String {
        let start = self.events.len().saturating_sub(n);
        self.events[start..]
            .iter()
            .filter_map(|e| serde_json::to_string(e).ok())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Return the last `n` events as a cloned `Vec<JournalEvent>`.
    /// Use for in-process consumers (metrics, TUI, etc.).
    pub fn recent(&self, n: usize) -> Vec<JournalEvent> {
        let start = self.events.len().saturating_sub(n);
        self.events[start..].to_vec()
    }

    /// Total event count in this journal (for metrics / dashboard display).
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Returns true if the journal is empty (required by clippy alongside `len()`).
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}
