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

use crate::plan::TransitionState;
use crate::types::CapabilityState;

pub use korg_core::ContentRef;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "event_type")]
pub enum CapabilityEvent {
    // ── Internal korg governance events ─────────────────────────────────────
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

    // ── Universal external agent event (schema v1.0) ─────────────────────────
    //
    // Any agent runtime — korgex, Claude Code, Codex, Amp, or a future MCP
    // client — can emit this event into a korg ledger. It is the primary
    // surface the MCP server will expose once the schema is frozen.
    //
    // Design rules (do not relax without bumping schema_version):
    //   1. Large payloads go in `payload_refs`, not inline in `args`/`result`.
    //   2. `source_agent` identifies the runtime, not the model.
    //   3. `triggered_by` on EventMetadata carries the causal seq_id pointer.
    AgentToolCall {
        /// Identity of the emitting agent runtime.
        /// Convention: lowercase, hyphenated. e.g. "korgex", "claude-code", "korg".
        source_agent: String,
        /// Name of the tool called. Should match the agent's own tool registry name.
        /// e.g. "Edit", "Bash", "Read", "korg_append_event".
        tool_name: String,
        /// Tool arguments. Keep small/scalar values inline.
        /// For large values (file contents, diffs), use payload_refs and record a
        /// ContentRef here instead: { "_ref": "sha256:<digest>" }.
        args: serde_json::Value,
        /// Tool result. Same content-addressing convention as args.
        result: serde_json::Value,
        /// Content-addressed references for any large payloads associated with
        /// this tool call (file contents, command output, diffs, etc.).
        #[serde(default)]
        payload_refs: Vec<ContentRef>,
        /// Whether the tool call succeeded.
        success: bool,
        /// Wall-clock duration of the tool call in milliseconds.
        duration_ms: u64,
        /// ISO-8601 timestamp of when the tool call was issued.
        timestamp: DateTime<Utc>,
    },
    ProxyAuditTrail {
        user_id: String,
        subscription_tier: String,
        model: String,
        estimated_input_tokens: u64,
        estimated_cost_usd: f64,
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
            // External agent events have no plan_id — use nil UUID as a stable sentinel
            CapabilityEvent::AgentToolCall { .. } | CapabilityEvent::ProxyAuditTrail { .. } => {
                Uuid::nil()
            }
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
            | CapabilityEvent::LeaseReleased { .. }
            | CapabilityEvent::ProxyAuditTrail { .. } => EventTier::Governance,

            CapabilityEvent::EffectStarted { .. }
            | CapabilityEvent::EffectCompleted { .. }
            | CapabilityEvent::EffectFailed { .. }
            | CapabilityEvent::EffectRetrying { .. } => EventTier::Effect,

            // External agent tool calls are telemetry-tier: high-volume, low-privilege
            CapabilityEvent::AgentToolCall { .. } => EventTier::Telemetry,
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

    /// Sequence number of the event that causally triggered this one.
    ///
    /// More human-readable than `causation_id` (UUID) for external consumers
    /// and auditors. When set, reading the ledger entry at `triggered_by` tells
    /// you *why* this event happened — the full causal chain is walkable by
    /// following seq_id pointers back to the root.
    ///
    /// Example: if seq=47 is an `AgentToolCall { tool_name: "Edit" }` that
    /// happened because seq=30 produced a failing test, set `triggered_by = Some(30)`.
    #[serde(default)]
    pub triggered_by: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JournalEvent {
    /// Schema version for forward compatibility.
    /// All events written by this release carry "1.0".
    /// Consumers MUST reject events with an unrecognised version.
    /// Bump to "1.1" for additive changes, "2.0" for breaking changes.
    #[serde(default = "JournalEvent::default_schema_version")]
    pub schema_version: String,
    pub seq_id: u64,
    pub metadata: EventMetadata,
    pub event: CapabilityEvent,
}

impl JournalEvent {
    pub fn default_schema_version() -> String {
        "1.0".to_string()
    }
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
    pub triggered_by_index: std::collections::HashMap<u64, Vec<usize>>,
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
            triggered_by_index: std::collections::HashMap::new(),
        };
        let _ = journal.load();
        journal
    }

    pub fn default_journal() -> Self {
        let root = korg_core::paths::project_root().join(".korg");
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
        let start_time = std::time::Instant::now();
        let lock_file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&self.lock_path)
            .map_err(|e| e.to_string())?;

        // Propagate lock failure. A previous version used .is_ok() and
        // proceeded either way, which meant load() returned silently with
        // empty in-memory state when another process held the lock — same
        // shape of "API claims success, disk wasn't read" as the speculative
        // flush bug. Refuse to claim success unless we actually held the lock.
        lock_file
            .lock_shared()
            .map_err(|e| format!("ledger shared lock failed: {e}"))?;

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

        self.rebuild_triggered_by_index();

        let elapsed = start_time.elapsed().as_millis();
        eprintln!(
            "journal load: {} events in {}ms",
            self.events.len(),
            elapsed
        );

        let _ = lock_file.unlock();
        Ok(())
    }

    /// Append event with full custom metadata control.
    ///
    /// **Concurrency invariant:** the `&mut self` receiver makes this function
    /// atomic from the borrow-checker's perspective — two threads cannot hold
    /// `&mut self` simultaneously. The production wiring wraps each
    /// `CapabilityJournal` in `Arc<tokio::sync::Mutex<CapabilityResolver>>` so
    /// the increment-then-assign pair below runs under the same lock. If a
    /// future refactor exposes interior mutability (e.g., `&self` with a
    /// `Cell<u64>`), the seq_id assignment must move to `AtomicU64::fetch_add`
    /// to keep this invariant.
    pub fn append_with_metadata(&mut self, event: CapabilityEvent, metadata: EventMetadata) {
        self.last_seq_id += 1;
        // Merge clock to maintain local HLC monotonicity
        let wall_clock = chrono::Utc::now().timestamp_millis();
        self.clock = self.clock.merge(&metadata.emitted_at, wall_clock);

        let journal_event = JournalEvent {
            schema_version: JournalEvent::default_schema_version(),
            seq_id: self.last_seq_id,
            metadata,
            event,
        };
        if let Some(tb) = journal_event.metadata.triggered_by {
            self.triggered_by_index
                .entry(tb)
                .or_insert_with(Vec::new)
                .push(self.events.len());
        }
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
        let triggered_by = parent.map(|e| e.seq_id);
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
            triggered_by,
        };

        self.last_seq_id += 1;
        self.clock = emitted_at;
        let journal_event = JournalEvent {
            schema_version: JournalEvent::default_schema_version(),
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

        self.rebuild_triggered_by_index();

        self.flush()?;
        Ok(())
    }

    /// Flush history to disk atomically with exclusive advisory locking.
    ///
    /// **Important:** when `is_speculative` is true (preview/dry-run mode),
    /// this is a deliberate no-op — no disk writes occur and `Ok(())` is
    /// returned even though nothing was persisted. Callers that *must*
    /// persist regardless of the speculative flag should call
    /// [`Self::force_flush`] instead.
    pub fn flush(&self) -> Result<(), String> {
        if self.is_speculative {
            return Ok(()); // intentional: preview mode never writes to disk
        }
        self.force_flush()
    }

    /// Flush history to disk regardless of the `is_speculative` flag.
    ///
    /// Use this when persistence is required even from speculative contexts
    /// (e.g. test teardown that needs to checkpoint state, or an explicit
    /// "commit my preview" action). Normal callers should use [`Self::flush`].
    pub fn force_flush(&self) -> Result<(), String> {
        let lock_file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&self.lock_path)
            .map_err(|e| e.to_string())?;

        // Lock failure used to be swallowed via .is_ok() — flush would return
        // Ok without ever touching disk. Propagate it instead so callers know
        // their write didn't land.
        lock_file
            .lock_exclusive()
            .map_err(|e| format!("ledger exclusive lock failed: {e}"))?;

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

    /// Rebuild the triggers mapping index.
    /// TODO: Incremental index persistence in v2 if startup time exceeds 100ms.
    pub fn rebuild_triggered_by_index(&mut self) {
        let mut index = std::collections::HashMap::new();
        for (idx, event) in self.events.iter().enumerate() {
            if let Some(tb) = event.metadata.triggered_by {
                index.entry(tb).or_insert_with(Vec::new).push(idx);
            }
        }
        self.triggered_by_index = index;
    }

    /// Return the last `n` events matching the trigger filter.
    /// Returns `None` if the specific trigger ID does not exist in the index to prevent ambiguity.
    pub fn to_json_lines_filtered(&self, triggered_by: Option<u64>, n: usize) -> Option<String> {
        match triggered_by {
            Some(tb) => {
                if let Some(indices) = self.triggered_by_index.get(&tb) {
                    let start = indices.len().saturating_sub(n);
                    let jsonl = indices[start..]
                        .iter()
                        .filter_map(|&idx| self.events.get(idx))
                        .filter_map(|e| serde_json::to_string(e).ok())
                        .collect::<Vec<_>>()
                        .join("\n");
                    Some(jsonl)
                } else {
                    None
                }
            }
            None => Some(self.to_json_lines(n)),
        }
    }

    /// Verify ledger integrity, ensuring any referenced blobs exist on disk in the blobs_dir.
    ///
    /// NOTE: This is a post-facto integrity check. It ensures that completed logs contain
    /// no dangling content references. Durable blob-first atomicity (ensuring that a blob
    /// is written with fsync *before* the event is appended to the ledger) must be enforced by the
    /// client writer's sequence of operations.
    ///
    /// Honest compliance statement: We verify integrity after the fact and abort replay on missing
    /// blobs, but we do not end-to-end simulate or test process crash-recovery during the split-second
    /// window between blob write and event append.
    pub fn verify_integrity(&self, blobs_dir: &std::path::Path) -> Result<(), String> {
        for event in &self.events {
            if let CapabilityEvent::AgentToolCall { payload_refs, .. } = &event.event {
                for content_ref in payload_refs {
                    let sha256 = &content_ref.sha256;
                    if sha256.len() < 2 {
                        return Err(format!("Malformed sha256: {}", sha256));
                    }
                    let prefix = &sha256[..2];
                    let blob_path = blobs_dir.join(prefix).join(sha256);
                    if !blob_path.exists() {
                        return Err(format!(
                            "Ledger integrity failure: missing blob for sha256: {}",
                            sha256
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}
