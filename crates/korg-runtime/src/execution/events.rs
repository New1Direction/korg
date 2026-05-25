//! Stable event types for the thump harness NDJSON protocol.
//!
//! Mirrors the Python `thump/events.py` contract so the two sides evolve
//! together with minimal friction.

use serde::{Deserialize, Serialize};

/// Core event envelope emitted by the Python harness for every intermediate step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BunEvent {
    /// ISO-8601 timestamp (with timezone)
    pub ts: String,

    /// Event name, e.g. "script.run.started", "package.add.process.stdout"
    pub event: String,

    /// Schema version for forward compatibility. Currently always "1".
    #[serde(default = "default_schema_version")]
    pub schema_version: String,

    /// Optional correlation ID for a whole user session / job
    #[serde(default)]
    pub session_id: Option<String>,

    /// Optional correlation ID for a single finite operation
    #[serde(default)]
    pub op_id: Option<String>,

    pub level: EventLevel,

    /// Arbitrary payload. Consumers should treat unknown fields gracefully.
    #[serde(default)]
    pub data: serde_json::Value,
}

fn default_schema_version() -> String {
    "1".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl EventLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            EventLevel::Debug => "debug",
            EventLevel::Info => "info",
            EventLevel::Warn => "warn",
            EventLevel::Error => "error",
        }
    }
}

/// Final structured outcome emitted by the harness (via `emit_result`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BunOutcome {
    pub ok: bool,
    pub operation: String,
    #[serde(flatten)]
    pub data: serde_json::Value,
}

/// Either a live event or a final outcome — the item type when streaming from the harness.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BunEventOrOutcome {
    Event(BunEvent),
    Outcome(BunOutcome),
}

impl BunEventOrOutcome {
    pub fn from_json_line(line: &str) -> Result<Self, serde_json::Error> {
        if let Ok(ev) = serde_json::from_str::<BunEvent>(line) {
            if !ev.event.is_empty() {
                return Ok(BunEventOrOutcome::Event(ev));
            }
        }
        let outcome: BunOutcome = serde_json::from_str(line)?;
        Ok(BunEventOrOutcome::Outcome(outcome))
    }
}
