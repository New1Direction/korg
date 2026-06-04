//! Agent-native introspection — emits a `korg:introspect@v1` document
//! describing every subcommand the `korg` binary exposes, with capability
//! metadata and stable command IDs.
//!
//! Shares the document format with:
//!   - `Korg/adapters/recall-mcp/src/korg_recall_mcp/introspect.py`
//!   - `API/thumper/src/cli/introspect.rs`
//!
//! Cross-language agents see one schema (`korg:introspect@v1`) across the
//! entire ecosystem.

use serde::Serialize;
use std::collections::BTreeMap;

pub const INTROSPECT_SCHEMA_ID: &str = "korg:introspect@v1";
pub const BINARY_NAME: &str = "korg";

#[derive(Debug, Serialize, Clone)]
pub struct Capabilities {
    pub output_mode: String,
    pub side_effects: String,
    pub requires_project: bool,
    pub long_running: bool,
    pub stateful: bool,
    pub reads_stdin: bool,
    pub supports_output_path: bool,
}

impl Capabilities {
    /// Conservative defaults — zero-effect, fast, no stdin.
    pub fn safe() -> Self {
        Self {
            output_mode: "envelope".to_string(),
            side_effects: "none".to_string(),
            requires_project: false,
            long_running: false,
            stateful: false,
            reads_stdin: false,
            supports_output_path: false,
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct Callable {
    pub command_id: String,
    pub name: String,
    pub description: String,
    pub surfaces: Vec<String>,
    pub input_schema: serde_json::Value,
    pub capabilities: Capabilities,
}

#[derive(Debug, Serialize)]
pub struct IntrospectDocument {
    pub schema: String,
    pub binary: String,
    pub version: String,
    pub callables_declared: bool,
    pub callables: Vec<Callable>,
    pub exit_codes: BTreeMap<String, String>,
}

pub fn exit_codes() -> BTreeMap<String, String> {
    [
        ("0", "success"),
        ("1", "error.generic"),
        ("2", "error.usage"),
        ("3", "error.config"),
        ("4", "error.io"),
        ("5", "error.network"),
        ("6", "error.user_interrupt"),
        ("7", "error.dependency_missing"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
    .collect()
}

pub fn callables() -> Vec<Callable> {
    vec![
        Callable {
            command_id: "korg.worker".to_string(),
            name: "worker".to_string(),
            description: "Run as a swarm worker, connecting to a leader \
                          orchestrator over the chosen transport (stdio / network)."
                .to_string(),
            surfaces: vec!["cli".to_string()],
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "default": "worker-01"},
                    "endpoint": {"type": "string", "default": "stdio"}
                }
            }),
            capabilities: Capabilities {
                output_mode: "session".to_string(),
                side_effects: "network".to_string(),
                long_running: true,
                stateful: true,
                reads_stdin: true,
                ..Capabilities::safe()
            },
        },
        Callable {
            command_id: "korg.leader".to_string(),
            name: "leader".to_string(),
            description: "Run as the leader orchestrator — schedules sub-agents \
                          via the execution DAG, runs human approval gates, \
                          writes the full HLC-ordered event log."
                .to_string(),
            surfaces: vec!["cli".to_string()],
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "endpoint": {"type": "string", "default": "stdio"},
                    "approve_all": {"type": "boolean", "default": false},
                    "mode": {"type": "string", "default": "balanced"}
                }
            }),
            capabilities: Capabilities {
                output_mode: "session".to_string(),
                side_effects: "ledger_write".to_string(),
                long_running: true,
                stateful: true,
                reads_stdin: true,
                ..Capabilities::safe()
            },
        },
        Callable {
            command_id: "korg.campaign".to_string(),
            name: "campaign".to_string(),
            description: "Run a full Heavy-Adversarial campaign over a prompt. \
                          Multi-persona swarm with HLC-ordered ledger, optional \
                          TUI dashboard, optional --web cockpit."
                .to_string(),
            surfaces: vec!["cli".to_string()],
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "prompt": {"type": "string"},
                    "tui": {"type": "boolean", "default": false},
                    "web": {"type": "boolean", "default": false},
                    "mode": {"type": "string", "default": "balanced"},
                    "preview": {"type": "boolean", "default": false}
                },
                "required": ["prompt"]
            }),
            capabilities: Capabilities {
                output_mode: "stream".to_string(),
                side_effects: "ledger_write".to_string(),
                long_running: true,
                stateful: true,
                ..Capabilities::safe()
            },
        },
        Callable {
            command_id: "korg.tui".to_string(),
            name: "tui".to_string(),
            description: "Launch the Ratatui operator dashboard against the \
                          live korg session ledger. --monitor-only renders \
                          without accepting steering input."
                .to_string(),
            surfaces: vec!["cli".to_string()],
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "monitor_only": {"type": "boolean", "default": false}
                }
            }),
            capabilities: Capabilities {
                output_mode: "none".to_string(),
                side_effects: "fs_read".to_string(),
                long_running: true,
                reads_stdin: true,
                ..Capabilities::safe()
            },
        },
        Callable {
            command_id: "korg.reconcile".to_string(),
            name: "reconcile".to_string(),
            description: "Run post-campaign factual reconciliation against \
                          a topic. Reads the ledger, scores claims, writes \
                          a reconciliation summary back into the ledger."
                .to_string(),
            surfaces: vec!["cli".to_string()],
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "topic": {"type": "string"}
                },
                "required": ["topic"]
            }),
            capabilities: Capabilities {
                output_mode: "envelope".to_string(),
                side_effects: "ledger_write".to_string(),
                long_running: true,
                ..Capabilities::safe()
            },
        },
        Callable {
            command_id: "korg.synthesize".to_string(),
            name: "synthesize".to_string(),
            description: "Run post-campaign concept synthesis — extracts the \
                          durable concepts from a campaign's ledger and writes \
                          them back as synthesis events."
                .to_string(),
            surfaces: vec!["cli".to_string()],
            input_schema: serde_json::json!({"type": "object"}),
            capabilities: Capabilities {
                output_mode: "envelope".to_string(),
                side_effects: "ledger_write".to_string(),
                long_running: true,
                ..Capabilities::safe()
            },
        },
        Callable {
            command_id: "korg.verify-provenance".to_string(),
            name: "verify-provenance".to_string(),
            description: "Cryptographically verify the Ed25519 attestation \
                          chain on a `.ktrans` transaction ledger."
                .to_string(),
            surfaces: vec!["cli".to_string()],
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                },
                "required": ["path"]
            }),
            capabilities: Capabilities {
                output_mode: "envelope".to_string(),
                side_effects: "fs_read".to_string(),
                ..Capabilities::safe()
            },
        },
        Callable {
            command_id: "korg.index".to_string(),
            name: "index".to_string(),
            description: "Build / refresh the semantic vector index for a \
                          workspace (Tree-sitter + Candle embeddings)."
                .to_string(),
            surfaces: vec!["cli".to_string()],
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "default": "."}
                }
            }),
            capabilities: Capabilities {
                output_mode: "envelope".to_string(),
                side_effects: "fs_write".to_string(),
                long_running: true,
                supports_output_path: true,
                ..Capabilities::safe()
            },
        },
        Callable {
            command_id: "korg.shell".to_string(),
            name: "shell".to_string(),
            description: "Start the interactive developer shell — REPL with \
                          /read, /edit, /goal, /reconcile slash commands."
                .to_string(),
            surfaces: vec!["cli".to_string()],
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "mode": {"type": "string", "default": "balanced"}
                }
            }),
            capabilities: Capabilities {
                output_mode: "session".to_string(),
                side_effects: "ledger_write".to_string(),
                long_running: true,
                stateful: true,
                reads_stdin: true,
                ..Capabilities::safe()
            },
        },
        Callable {
            command_id: "korg.lsp".to_string(),
            name: "lsp".to_string(),
            description: "Run as a read-only Language Server Protocol server \
                          over stdio — semantic navigation over the ledger."
                .to_string(),
            surfaces: vec!["cli".to_string(), "lsp".to_string()],
            input_schema: serde_json::json!({"type": "object"}),
            capabilities: Capabilities {
                output_mode: "session".to_string(),
                side_effects: "fs_read".to_string(),
                long_running: true,
                reads_stdin: true,
                ..Capabilities::safe()
            },
        },
        Callable {
            command_id: "korg.rewind".to_string(),
            name: "rewind".to_string(),
            description: "Deterministically restore the workspace + ledger to \
                          a specific sequence point (O(1) via git read-tree)."
                .to_string(),
            surfaces: vec!["cli".to_string()],
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "seq": {"type": "integer", "description": "Target ledger sequence."}
                },
                "required": ["seq"]
            }),
            capabilities: Capabilities {
                output_mode: "envelope".to_string(),
                side_effects: "ledger_write".to_string(),
                stateful: true,
                ..Capabilities::safe()
            },
        },
        Callable {
            command_id: "korg.demo".to_string(),
            name: "demo".to_string(),
            description: "Run the built-in cognitive time-travel demo — \
                          shows an agent fail, rewind, and re-execute correctly."
                .to_string(),
            surfaces: vec!["cli".to_string()],
            input_schema: serde_json::json!({"type": "object"}),
            capabilities: Capabilities {
                output_mode: "stream".to_string(),
                long_running: true,
                ..Capabilities::safe()
            },
        },
        Callable {
            command_id: "korg.auth".to_string(),
            name: "auth".to_string(),
            description: "Manage agent OAuth credentials (login / status / logout) \
                          for providers like Codex, Claude, etc."
                .to_string(),
            surfaces: vec!["cli".to_string()],
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "subcommand": {
                        "type": "string",
                        "enum": ["login", "status", "logout"]
                    },
                    "provider": {"type": "string", "default": "codex"}
                }
            }),
            capabilities: Capabilities {
                output_mode: "envelope".to_string(),
                side_effects: "fs_write".to_string(),
                ..Capabilities::safe()
            },
        },
    ]
}

pub fn build_document(version: &str) -> IntrospectDocument {
    IntrospectDocument {
        schema: INTROSPECT_SCHEMA_ID.to_string(),
        binary: BINARY_NAME.to_string(),
        version: version.to_string(),
        callables_declared: true,
        callables: callables(),
        exit_codes: exit_codes(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_has_schema_tag() {
        let doc = build_document("0.1.0");
        assert_eq!(doc.schema, "korg:introspect@v1");
    }

    #[test]
    fn document_carries_binary_and_version() {
        let doc = build_document("9.9.9");
        assert_eq!(doc.binary, "korg");
        assert_eq!(doc.version, "9.9.9");
        assert!(doc.callables_declared);
    }

    #[test]
    fn callables_have_unique_ids() {
        let ids: Vec<_> = callables().iter().map(|c| c.command_id.clone()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "duplicate command_ids: {:?}", ids);
    }

    #[test]
    fn all_command_ids_namespaced() {
        for c in callables() {
            assert!(
                c.command_id.starts_with("korg."),
                "command_id must start with 'korg.': {}",
                c.command_id
            );
            assert!(
                !c.command_id.contains(' '),
                "command_id must not contain spaces: {}",
                c.command_id
            );
        }
    }

    #[test]
    fn recognized_side_effects() {
        let valid = ["none", "fs_read", "fs_write", "network", "ledger_write"];
        for c in callables() {
            assert!(
                valid.contains(&c.capabilities.side_effects.as_str()),
                "unknown side_effects on {}: {}",
                c.command_id,
                c.capabilities.side_effects
            );
        }
    }

    #[test]
    fn recognized_output_modes() {
        let valid = ["none", "stream", "envelope", "session"];
        for c in callables() {
            assert!(
                valid.contains(&c.capabilities.output_mode.as_str()),
                "unknown output_mode on {}: {}",
                c.command_id,
                c.capabilities.output_mode
            );
        }
    }

    #[test]
    fn input_schemas_are_object_typed() {
        for c in callables() {
            assert_eq!(
                c.input_schema.get("type").and_then(|v| v.as_str()),
                Some("object"),
                "input_schema must be 'type: object' for {}",
                c.command_id
            );
        }
    }

    #[test]
    fn long_running_stateful_uses_session_or_stream_output() {
        for c in callables() {
            if c.capabilities.long_running && c.capabilities.stateful {
                assert!(
                    matches!(
                        c.capabilities.output_mode.as_str(),
                        "session" | "stream" | "none"
                    ),
                    "{} long_running+stateful but output_mode={}",
                    c.command_id,
                    c.capabilities.output_mode
                );
            }
        }
    }

    #[test]
    fn document_round_trips_through_json() {
        let doc = build_document("0.1.0");
        let blob = serde_json::to_string(&doc).expect("serialize");
        let v: serde_json::Value = serde_json::from_str(&blob).expect("parse");
        assert_eq!(v["schema"], "korg:introspect@v1");
        assert_eq!(v["binary"], "korg");
        assert_eq!(v["callables_declared"], true);
        assert!(v["callables"].is_array());
        assert!(v["exit_codes"].is_object());
    }

    #[test]
    fn exit_codes_table_is_complete_and_string_keyed() {
        let codes = exit_codes();
        assert_eq!(codes.get("0").map(|s| s.as_str()), Some("success"));
        for key in codes.keys() {
            assert!(
                key.parse::<u32>().is_ok(),
                "non-numeric exit-code key: {}",
                key
            );
        }
    }

    #[test]
    fn matches_thumper_and_recall_mcp_schema_id() {
        // Cross-ecosystem invariant: every korg adapter / binary that emits
        // an introspect document must use the same schema ID. If we ever
        // bump the schema, every adapter must bump in lockstep.
        assert_eq!(INTROSPECT_SCHEMA_ID, "korg:introspect@v1");
    }
}
