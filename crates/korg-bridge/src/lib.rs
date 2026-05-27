//! korg-bridge — in-process Python bridge to Korg's WAL.
//!
//! Exposes a single Python type: [`Bridge`]. The Bridge wraps a
//! [`korg_registry::CapabilityJournal`] and offers three append methods that
//! match what `korgex/src/korg_ledger.py` previously POSTed over HTTP:
//!
//!   * `record_user_prompt(prompt)`        — root event, triggered_by=None
//!   * `record_llm_call(model, ...)`       — chained to the previous LLM round
//!   * `record_tool_call(tool, args, ...)` — chained to whatever the caller passes
//!
//! Each method returns the assigned `seq_id` so the next call can chain its
//! `triggered_by` correctly.
//!
//! The on-disk format is identical to what `korg-server`'s HTTP handler writes,
//! so a server can be launched against the same journal after the fact for
//! MCP serving / browsing — there's no schema fork.

use std::path::PathBuf;
use std::sync::Mutex;

use chrono::Utc;
use korg_registry::log::{EventMetadata, EventTier};
use korg_registry::{CapabilityEvent, CapabilityJournal, ContentRef};
use pyo3::exceptions::{PyOSError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyAny;
use pythonize::depythonize;
use uuid::Uuid;

/// Default snapshot interval — matches the value korg-server uses when
/// constructing its journal.
const DEFAULT_SNAPSHOT_INTERVAL: usize = 100;

/// `Bridge` owns a `CapabilityJournal` behind a Mutex so multiple Python
/// threads can call the append methods serially without trampling each
/// other's in-memory state.
///
/// The Mutex is the *intra-process* serialiser. The journal's own file lock
/// is the *inter-process* serialiser — both apply.
#[pyclass(module = "korg_bridge")]
struct Bridge {
    journal: Mutex<CapabilityJournal>,
}

#[pymethods]
impl Bridge {
    /// Construct a Bridge against the given journal path.
    ///
    /// If `snapshot_path` or `lock_path` are omitted, they default to
    /// `<journal>.snapshot.json` and `<journal>.lock` next to the journal.
    ///
    /// On construction we call `journal.load()` so the in-memory state matches
    /// what's already on disk. A missing file is fine — we start empty.
    #[new]
    #[pyo3(signature = (
        journal_path,
        snapshot_path = None,
        lock_path = None,
        snapshot_interval = DEFAULT_SNAPSHOT_INTERVAL,
    ))]
    fn new(
        journal_path: PathBuf,
        snapshot_path: Option<PathBuf>,
        lock_path: Option<PathBuf>,
        snapshot_interval: usize,
    ) -> PyResult<Self> {
        // Derive sibling paths when not provided. We avoid `with_extension`
        // for the snapshot/lock siblings because that drops information for
        // paths like "journal.json" → ".lock".json.
        let snapshot_path = snapshot_path.unwrap_or_else(|| {
            let mut p = journal_path.clone();
            let stem = p
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "journal".to_string());
            p.set_file_name(format!("{stem}.snapshot.json"));
            p
        });
        let lock_path = lock_path.unwrap_or_else(|| {
            let mut p = journal_path.clone();
            let stem = p
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "journal".to_string());
            p.set_file_name(format!("{stem}.lock"));
            p
        });

        let mut journal = CapabilityJournal::new(
            journal_path,
            snapshot_path,
            snapshot_interval,
            lock_path,
        );
        // load() will succeed on a missing file (it checks .exists()), so we
        // only surface real errors (lock contention, malformed JSON, etc.).
        journal.load().map_err(PyOSError::new_err)?;

        Ok(Bridge {
            journal: Mutex::new(journal),
        })
    }

    /// Emit a root `user_prompt` event. Returns the assigned `seq_id`, which
    /// the caller passes as `triggered_by` on the next event.
    fn record_user_prompt(&self, prompt: &str) -> PyResult<u64> {
        let args = serde_json::json!({ "prompt": prompt });
        let result = serde_json::json!({ "success": true });
        self.append(
            "human:claude-code-user",
            "user_prompt",
            args,
            result,
            true,
            0,
            None,
        )
    }

    /// Emit an `llm_inference` event. Returns the assigned `seq_id`.
    #[pyo3(signature = (model, prompt_tokens, completion_tokens, duration_ms, triggered_by, source_agent = "agent:korgex@0.3.0"))]
    fn record_llm_call(
        &self,
        model: &str,
        prompt_tokens: u64,
        completion_tokens: u64,
        duration_ms: u64,
        triggered_by: Option<u64>,
        source_agent: &str,
    ) -> PyResult<u64> {
        let args = serde_json::json!({
            "model": model,
            "prompt_tokens": prompt_tokens,
        });
        let result = serde_json::json!({
            "completion_tokens": completion_tokens,
        });
        self.append(
            source_agent,
            "llm_inference",
            args,
            result,
            true,
            duration_ms,
            triggered_by,
        )
    }

    /// Emit a generic tool-call event. Args and result are arbitrary
    /// JSON-serialisable Python objects (dict, list, str, int, bool, None);
    /// they're converted via `pythonize::depythonize`.
    #[pyo3(signature = (source_agent, tool_name, args, result, success, duration_ms, triggered_by = None))]
    fn record_tool_call(
        &self,
        source_agent: &str,
        tool_name: &str,
        args: &Bound<'_, PyAny>,
        result: &Bound<'_, PyAny>,
        success: bool,
        duration_ms: u64,
        triggered_by: Option<u64>,
    ) -> PyResult<u64> {
        let args_json: serde_json::Value =
            depythonize(args).map_err(|e| PyTypeError::new_err(format!("args: {e}")))?;
        let result_json: serde_json::Value =
            depythonize(result).map_err(|e| PyTypeError::new_err(format!("result: {e}")))?;
        self.append(
            source_agent,
            tool_name,
            args_json,
            result_json,
            success,
            duration_ms,
            triggered_by,
        )
    }

    /// Return the highest seq_id currently in the journal.
    fn last_seq_id(&self) -> PyResult<u64> {
        let j = self
            .journal
            .lock()
            .map_err(|e| PyOSError::new_err(format!("journal mutex poisoned: {e}")))?;
        Ok(j.last_seq_id)
    }

    /// Force a flush to disk. Normally not needed — append_with_metadata
    /// flushes inside the journal — but exposed for tests and explicit
    /// shutdown paths.
    fn flush(&self) -> PyResult<()> {
        let j = self
            .journal
            .lock()
            .map_err(|e| PyOSError::new_err(format!("journal mutex poisoned: {e}")))?;
        j.flush().map_err(PyOSError::new_err)
    }

    fn __repr__(&self) -> PyResult<String> {
        let j = self
            .journal
            .lock()
            .map_err(|e| PyOSError::new_err(format!("journal mutex poisoned: {e}")))?;
        Ok(format!(
            "<korg_bridge.Bridge events={} last_seq_id={}>",
            j.events.len(),
            j.last_seq_id
        ))
    }
}

impl Bridge {
    /// Core append path — mirrors the HTTP `agent_tool_call_handler` in
    /// `korg-server/src/lib.rs:790-877`. Held in a single helper so the
    /// three Python-facing methods can share the (long) metadata construction.
    #[allow(clippy::too_many_arguments)]
    fn append(
        &self,
        source_agent: &str,
        tool_name: &str,
        args: serde_json::Value,
        result: serde_json::Value,
        success: bool,
        duration_ms: u64,
        triggered_by: Option<u64>,
    ) -> PyResult<u64> {
        let mut journal = self
            .journal
            .lock()
            .map_err(|e| PyOSError::new_err(format!("journal mutex poisoned: {e}")))?;

        if duration_ms > u64::MAX / 2 {
            // Defensive: silently coerce nothing; surface absurd values loudly
            // so a Python-side bug doesn't poison the journal with negative-
            // looking i64s.
            return Err(PyValueError::new_err(format!(
                "duration_ms {duration_ms} is absurdly large; refusing to record",
            )));
        }

        let event = CapabilityEvent::AgentToolCall {
            source_agent: source_agent.to_string(),
            tool_name: tool_name.to_string(),
            args,
            result,
            payload_refs: Vec::<ContentRef>::new(),
            success,
            duration_ms,
            timestamp: Utc::now(),
        };

        // Build EventMetadata exactly the way agent_tool_call_handler does.
        // This is the place that the dogfood audit (2026-05-24) flagged: if
        // we let CapabilityJournal::append() auto-fill triggered_by we'd
        // chain to whatever internal korg event happened to be last,
        // breaking the causal tree. Build metadata explicitly and call
        // append_with_metadata so the caller's triggered_by is preserved.
        let event_id = Uuid::new_v4();
        let wall_clock = Utc::now().timestamp_millis();
        let emitted_at = journal.clock.tick(wall_clock);

        let (root_event_id, causation_id) = match triggered_by {
            Some(triggered_by_seq) => {
                let parent = journal
                    .events
                    .iter()
                    .find(|e| e.seq_id == triggered_by_seq);
                let root = parent.map(|e| e.metadata.root_event_id).unwrap_or(event_id);
                let causation = parent.map(|e| e.metadata.event_id);
                (root, causation)
            }
            None => (event_id, None),
        };

        let metadata = EventMetadata {
            event_id,
            correlation_id: Uuid::nil(),
            causation_id,
            root_event_id,
            // Recorder identity — matches what korg-server's HTTP handler
            // uses for external agent events.
            actor_id: "korg:bridge".to_string(),
            campaign_id: Uuid::nil(),
            emitted_at,
            branch_id: None,
            speculative: false,
            retry_count: 0,
            tier: EventTier::Telemetry,
            span_id: None,
            tags: std::collections::BTreeMap::new(),
            triggered_by,
        };

        journal.append_with_metadata(event, metadata);
        Ok(journal.last_seq_id)
    }
}

/// Python module entrypoint. `maturin develop` produces `korg_bridge.so`
/// in the local site-packages; `import korg_bridge` loads this function.
#[pymodule]
fn korg_bridge(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Bridge>()?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
