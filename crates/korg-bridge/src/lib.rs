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

// CausalityError — raised when a write would violate the causal DAG.
//
// The bridge is the trust boundary where Python writes enter the Rust journal.
// `verify_dag` (korg-ledger@v1 §5) checks the same invariant at READ time, but
// by then a broken event is already chained. This exception lets the bridge
// REJECT the write before it lands: a `triggered_by` must name an existing,
// strictly-earlier `seq_id`. Subclasses `ValueError` so callers that only catch
// `ValueError` (the previous behaviour for bad input) still see it.
pyo3::create_exception!(
    korg_bridge,
    CausalityError,
    PyValueError,
    "A write whose triggered_by does not reference an existing, strictly-earlier seq_id."
);

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

        let mut journal =
            CapabilityJournal::new(journal_path, snapshot_path, snapshot_interval, lock_path);
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
            Vec::new(),
        )
    }

    /// Emit an `llm_inference` event. Returns the assigned `seq_id`.
    ///
    /// v0.3.2: `assistant_text` is optional. When provided, the model's
    /// reply text lands on the event's `result` field so downstream
    /// consumers (search, audit, replay) can find it. Token counts stay in
    /// the same places as before. Callers responsible for content-addressing
    /// large replies — the bridge writes whatever text it's given.
    #[pyo3(signature = (
        model,
        prompt_tokens,
        completion_tokens,
        duration_ms,
        triggered_by,
        source_agent = "agent:korgex@0.3.0",
        assistant_text = None,
    ))]
    fn record_llm_call(
        &self,
        model: &str,
        prompt_tokens: u64,
        completion_tokens: u64,
        duration_ms: u64,
        triggered_by: Option<u64>,
        source_agent: &str,
        assistant_text: Option<&str>,
    ) -> PyResult<u64> {
        let args = serde_json::json!({
            "model": model,
            "prompt_tokens": prompt_tokens,
        });
        // result carries completion_tokens + (since v0.3.2) optional text.
        // Keep the field name "text" — short, obvious, and matches what a
        // future content-addressed variant would also use as a key.
        let mut result_map = serde_json::Map::new();
        result_map.insert(
            "completion_tokens".to_string(),
            serde_json::Value::from(completion_tokens),
        );
        if let Some(text) = assistant_text {
            result_map.insert(
                "text".to_string(),
                serde_json::Value::String(text.to_string()),
            );
        }
        let result = serde_json::Value::Object(result_map);
        self.append(
            source_agent,
            "llm_inference",
            args,
            result,
            true,
            duration_ms,
            triggered_by,
            Vec::new(),
        )
    }

    /// Emit a generic tool-call event. Args and result are arbitrary
    /// JSON-serialisable Python objects (dict, list, str, int, bool, None);
    /// they're converted via `pythonize::depythonize`.
    ///
    /// `payload_refs` is an optional list of dicts shaped
    /// `{"sha256": str, "size_bytes": int, "label": str}` — content-addressed
    /// references for any large blobs the caller wrote out-of-band. Pass
    /// `None` or `[]` if the tool call has no content-addressed payloads.
    #[pyo3(signature = (source_agent, tool_name, args, result, success, duration_ms, triggered_by = None, payload_refs = None))]
    fn record_tool_call(
        &self,
        source_agent: &str,
        tool_name: &str,
        args: &Bound<'_, PyAny>,
        result: &Bound<'_, PyAny>,
        success: bool,
        duration_ms: u64,
        triggered_by: Option<u64>,
        payload_refs: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<u64> {
        let args_json: serde_json::Value =
            depythonize(args).map_err(|e| PyTypeError::new_err(format!("args: {e}")))?;
        let result_json: serde_json::Value =
            depythonize(result).map_err(|e| PyTypeError::new_err(format!("result: {e}")))?;
        let refs = parse_payload_refs(payload_refs)?;
        self.append(
            source_agent,
            tool_name,
            args_json,
            result_json,
            success,
            duration_ms,
            triggered_by,
            refs,
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

/// Parse a Python `list[{"sha256":..,"size_bytes":..,"label":..}]` (or None/[])
/// into a `Vec<ContentRef>`. Each entry's missing/wrong-typed fields raise
/// PyTypeError naming the field — easier to debug than a silent skip.
fn parse_payload_refs(refs: Option<&Bound<'_, PyAny>>) -> PyResult<Vec<ContentRef>> {
    let Some(refs) = refs else {
        return Ok(Vec::new());
    };
    if refs.is_none() {
        return Ok(Vec::new());
    }
    let value: serde_json::Value =
        depythonize(refs).map_err(|e| PyTypeError::new_err(format!("payload_refs: {e}")))?;
    let arr = match value {
        serde_json::Value::Null => return Ok(Vec::new()),
        serde_json::Value::Array(a) => a,
        other => {
            return Err(PyTypeError::new_err(format!(
                "payload_refs must be a list, got {}",
                other_type_name(&other)
            )))
        }
    };
    let mut out = Vec::with_capacity(arr.len());
    for (i, item) in arr.into_iter().enumerate() {
        let obj = match item {
            serde_json::Value::Object(m) => m,
            _ => {
                return Err(PyTypeError::new_err(format!(
                    "payload_refs[{i}] must be a dict"
                )))
            }
        };
        let sha256 = obj
            .get("sha256")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PyTypeError::new_err(format!("payload_refs[{i}].sha256 must be a str")))?
            .to_string();
        let size_bytes = obj
            .get("size_bytes")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                PyTypeError::new_err(format!(
                    "payload_refs[{i}].size_bytes must be a non-negative int"
                ))
            })?;
        let label = obj
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        out.push(ContentRef {
            sha256,
            size_bytes,
            label,
        });
    }
    Ok(out)
}

fn other_type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
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
        payload_refs: Vec<ContentRef>,
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

        // Write-time causality gate (korg-ledger@v1 §5, enforced at the trust
        // boundary). If the caller names a parent, it MUST exist and be
        // strictly earlier than the seq_id this write is about to take. We
        // reject BEFORE touching the clock or appending, so a rejected write
        // neither advances last_seq_id nor leaves a partial event on disk.
        if let Some(parent_seq) = triggered_by {
            // append_with_metadata assigns seq_id = last_seq_id + 1.
            let next_seq = journal.last_seq_id + 1;
            if parent_seq >= next_seq {
                return Err(CausalityError::new_err(format!(
                    "triggered_by {parent_seq} is not strictly earlier than this \
                     event's seq_id {next_seq}; refusing to record"
                )));
            }
            if !journal.events.iter().any(|e| e.seq_id == parent_seq) {
                return Err(CausalityError::new_err(format!(
                    "triggered_by {parent_seq} references no existing event; \
                     refusing to record an orphan"
                )));
            }
        }

        let event = CapabilityEvent::AgentToolCall {
            source_agent: source_agent.to_string(),
            tool_name: tool_name.to_string(),
            args,
            result,
            payload_refs,
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
                let parent = journal.events.iter().find(|e| e.seq_id == triggered_by_seq);
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
    m.add("CausalityError", m.py().get_type_bound::<CausalityError>())?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
