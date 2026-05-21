//! ACP v1.17 Wire Format (incremental implementation)
//!
//! Target: wiki/reference-harness/ACP-v1.17-Wire-Format.md
//!
//! This increment focuses on:
//! - JCS (RFC 8785) canonicalization
//! - Ed25519 signatures over the canonical form
//! - Strongly-typed messages for key operations
//! - Error taxonomy with retry semantics

use anyhow::Result;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Common envelope fields for every ACP message (per v1.17 spec).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope<P> {
    pub message_id: Uuid,
    pub timestamp: String, // RFC 3339
    pub sender: String,
    pub payload: P,
    pub signature: SignatureObject,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureObject {
    pub public_key: String,      // 64 hex chars
    pub signature_bytes: String, // 128 hex chars
}

/// ===== Payload Types (aligned with the spec) =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanPresentationPayload {
    pub plan_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub steps: Vec<PlanStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub step_id: String,
    pub name: String,
    pub description: String,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskApprovePayload {
    pub task_id: Uuid,
    pub approved: bool,
    pub comment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaResultPayload {
    pub arena_id: Uuid,
    pub execution_status: String,
    pub metrics: ArenaMetrics,
    pub error: Option<ArenaError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaMetrics {
    pub duration_ms: u64,
    pub cpu_utilization: f64,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaError {
    pub code: String,
    pub message: String,
}

/// For conflict.resolve
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictResolvePayload {
    pub conflict_id: Uuid,
    pub key: String,
    pub conflicting_outputs: Vec<serde_json::Value>,
    pub strategy: String, // "ours", "theirs", "merge", "arena", "human", etc.
    pub requires_human: bool,
}

/// For tool.invoke
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvokePayload {
    pub tool_call_id: Uuid,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub capability_scope: String,
    pub timeout_ms: Option<u64>,
}

/// Full error taxonomy aligned with Grok-native ACP v1.17.
/// Includes state_invalidation guidance for the harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpError {
    Prohibited { code: String, description: String },
    AuthFailure { code: String, description: String },
    RiskDetected { code: String, description: String },
    InvalidBaseSnapshot,
    ProvenanceMissing,
    HashMismatch,
    EpochExpired,
    CapabilityMismatch,
    PayloadTooLarge,
    SchemaViolation,
    DoomLoopDetected,
    MergeConflict,
    TaskTimeout,
    PermissionDenied,
    ContextEvicted,
    InternalError { message: String },
}

impl AcpError {
    pub fn is_retryable(&self) -> bool {
        match self {
            AcpError::Prohibited { .. } => false,
            AcpError::AuthFailure { .. } => true,
            AcpError::RiskDetected { .. } => true,
            AcpError::InvalidBaseSnapshot => false,
            AcpError::ProvenanceMissing => false,
            AcpError::HashMismatch => false,
            AcpError::EpochExpired => true,
            AcpError::CapabilityMismatch => false,
            AcpError::PayloadTooLarge => true,
            AcpError::SchemaViolation => false,
            AcpError::DoomLoopDetected => false,
            AcpError::MergeConflict => true,
            AcpError::TaskTimeout => true,
            AcpError::PermissionDenied => false,
            AcpError::ContextEvicted => true,
            AcpError::InternalError { .. } => true,
        }
    }

    pub fn requires_reauth(&self) -> bool {
        matches!(self, AcpError::AuthFailure { .. })
    }

    /// Returns guidance on what state should be invalidated when this error occurs.
    /// This directly supports the Korg epistemic state machine and .ktrans rollback logic.
    pub fn state_invalidation(&self) -> &'static str {
        match self {
            AcpError::Prohibited { .. } => "immediate local agent suspension + worktree discard; mark affected artifacts CONTESTED",
            AcpError::AuthFailure { .. } => "channel considered unauthenticated; pending frames dropped; re-auth required",
            AcpError::RiskDetected { .. } => "rollback to last verified checkpoint; partial .ktrans retained as provisional",
            AcpError::InvalidBaseSnapshot => "worker terminated; routing_id re-queued with corrected snapshot",
            AcpError::ProvenanceMissing => "transaction rejected; no merge queue entry created",
            AcpError::HashMismatch => "permanent rejection; offending agent capability may be reduced",
            AcpError::EpochExpired => "STALLED emitted; partial work preserved via last successful heartbeat",
            AcpError::CapabilityMismatch => "immediate revocation of offending capability",
            AcpError::PayloadTooLarge => "frame dropped; sender notified",
            AcpError::SchemaViolation => "message ignored",
            AcpError::DoomLoopDetected => "RequestTerminate sent; worker killed; productive partial .ktrans still accepted",
            AcpError::MergeConflict => "artifact remains CONTESTED until resolved via human review",
            AcpError::TaskTimeout => "subtree pruned; partial results retained",
            AcpError::PermissionDenied => "operation rejected; audit entry created",
            AcpError::ContextEvicted => "affected agent must rehydrate context before continuing",
            AcpError::InternalError { .. } => "best-effort checkpoint rollback; operator notification",
        }
    }
}

/// ===== Signing / Verification Helpers (JCS + Ed25519 per v1.17) =====
/// Canonicalizes a serializable value.
/// For the reference harness we use stable JSON (field order from serde + BTreeMap where needed).
/// A production implementation would use a true RFC 8785 JCS crate.
pub fn canonicalize<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    // Best-effort stable form for the skeleton (good enough for signing demo)
    let s = serde_json::to_string(value)?;
    Ok(s.into_bytes())
}

/// Creates a SignatureObject for a given payload using the provided signing key.
/// The signature is computed over the JCS-canonicalized payload (excluding any signature field).
pub fn sign_payload<T: Serialize>(
    signing_key: &SigningKey,
    payload: &T,
) -> Result<SignatureObject> {
    let canonical = canonicalize(payload)?;
    let signature = signing_key.sign(&canonical);

    Ok(SignatureObject {
        public_key: hex::encode(signing_key.verifying_key().to_bytes()),
        signature_bytes: hex::encode(signature.to_bytes()),
    })
}

/// Verifies that the signature on a MessageEnvelope is valid for its payload.
pub fn verify_envelope<P: Serialize + for<'de> Deserialize<'de>>(
    envelope: &MessageEnvelope<P>,
) -> Result<bool> {
    let pubkey_bytes: [u8; 32] = hex::decode(&envelope.signature.public_key)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid public key length"))?;

    let verifying_key = VerifyingKey::from_bytes(&pubkey_bytes)?;
    let sig_bytes: [u8; 64] = hex::decode(&envelope.signature.signature_bytes)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid signature length"))?;
    let signature = Signature::from_bytes(&sig_bytes);

    let canonical = canonicalize(&envelope.payload)?;

    Ok(verifying_key.verify_strict(&canonical, &signature).is_ok())
}

/// ===== High-level typed messages we use in the skeleton =====
pub type PlanPresentationMessage = MessageEnvelope<PlanPresentationPayload>;
pub type TaskApproveMessage = MessageEnvelope<TaskApprovePayload>;
pub type ArenaResultMessage = MessageEnvelope<ArenaResultPayload>;

/// Simple ACP client (still using stdio for the skeleton, but now with signing).
pub struct AcpClient {
    pub worker_id: String,
    signing_key: SigningKey,
    _endpoint: String,
}

impl AcpClient {
    /// Convenience constructor used by main.rs worker path.
    pub async fn connect(
        endpoint: &str,
        worker_id: &str,
        _capabilities: Vec<String>,
    ) -> Result<Self> {
        Ok(Self::new(worker_id, endpoint))
    }

    pub fn new(worker_id: &str, endpoint: &str) -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        Self {
            worker_id: worker_id.to_string(),
            signing_key,
            _endpoint: endpoint.to_string(),
        }
    }

    /// Construct an AcpClient that writes real signed MessageEnvelope<AcpMessage>
    /// to stdout (used by workers in the stdio child-process path).
    pub fn new_stdio(worker_id: &str, signing_key: SigningKey) -> Self {
        Self {
            worker_id: worker_id.to_string(),
            signing_key,
            _endpoint: "stdio".to_string(),
        }
    }

    /// Creates a signed message ready to be sent on the wire.
    pub fn create_signed_message<P: Serialize>(&self, payload: P) -> Result<MessageEnvelope<P>> {
        let message_id = Uuid::new_v4();
        let timestamp = chrono::Utc::now().to_rfc3339();

        let signature = sign_payload(&self.signing_key, &payload)?;

        Ok(MessageEnvelope {
            message_id,
            timestamp,
            sender: self.worker_id.clone(),
            payload,
            signature,
        })
    }

    /// Verifies and returns the inner payload if valid.
    pub fn verify_message<P: Serialize + for<'de> Deserialize<'de>>(
        msg: &MessageEnvelope<P>,
    ) -> Result<bool> {
        verify_envelope(msg)
    }

    // The rest of the transport (stdio) remains stubbed for this increment.
    // In a later pass we will add proper CRLF-delimited JCS framing + real I/O.

    /// Stub receive (used by SingleWorkerHarness in worker mode).
    pub async fn receive(&mut self) -> Result<AcpMessage> {
        // In real life this would read a line from stdin and deserialize.
        // For the current skeleton we synthesize a harmless RouteWork so the worker doesn't hang.
        Ok(AcpMessage::RouteWork {
            routing_id: "auto-stub".into(),
            capabilities: vec!["benjamin".into()],
            payload: "stub task from leader simulation".into(),
            base_snapshot: "genesis".into(),
            codebase_merkle_root: "sha256:codebase-fallback".into(),
            permissions: vec![],
        })
    }

    /// Send an AcpMessage.
    /// - In "stdio" mode (worker child processes): writes a real signed MessageEnvelope to stdout.
    /// - Otherwise: legacy stub (prints).
    pub async fn send(&self, msg: &AcpMessage) -> Result<()> {
        if self._endpoint == "stdio" {
            let mut stdout = tokio::io::stdout();
            write_signed_acp_envelope(&mut stdout, &self.signing_key, msg.clone()).await
        } else {
            println!(
                "[AcpClient] (stub) would send: {:?}",
                serde_json::to_string(msg).unwrap_or_default()
            );
            Ok(())
        }
    }
}

// =============================================================================
// ACP Message Types (used across harness + LeaderOrchestrator)
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AcpMessage {
    // Core routing & lifecycle (from earlier increments)
    RouteWork {
        routing_id: String,
        capabilities: Vec<String>,
        payload: String,
        base_snapshot: String,
        codebase_merkle_root: String,
        permissions: Vec<String>,
    },
    SubmitTransaction {
        tx_id: Uuid,
        content_hash: String,
        payload: serde_json::Value,
    },
    TerminationReport {
        routing_id: String,
        exit_status: String,
        final_ktrans: Option<serde_json::Value>,
        // Back-compat fields used by current harness
        worker_id: Option<String>,
        terminal_tx_id: Option<Uuid>,
    },
    PlanPresentation {
        task_id: Uuid,
        plan: serde_json::Value,
        requires_approval: bool,
    },

    // Evaluation & telemetry (Heavy-Tier guardrail layer)
    SwarmTelemetryPulse {
        agent_id: String,
        per_agent: serde_json::Value,
        aggregate: serde_json::Value,
        scaling_recommendation: Option<String>,
    },
    EvaluationVerdict {
        verdict_id: Uuid,
        session_id: Uuid,
        overall: String,
        passed_rubrics: u8,
        total_rubrics: u8,
        justifications: Vec<String>,
        recommended_action: String,
        semantic_entropy: f32,
        doom_loop_detected: bool,
    },
    RequestTerminate {
        reason: String,
        error_code: String,
        rollback_to_snapshot: Option<String>,
    },

    /// First-class ACP message for campaign transactional logs (.ktrans).
    /// Allows .ktrans to be routed, framed, and verified exactly like other ACP messages.
    CampaignKtrans { ktrans: CampaignKtrans },

    // === Coding Tool Payloads (foundational for Option C) ===
    FileReadRequest(FileReadRequestPayload),
    FileReadResult(FileReadResultPayload),

    ShellExecRequest(ShellExecRequestPayload),
    ShellExecResult(ShellExecResultPayload),

    CodeEditProposal(CodeEditProposalPayload),

    PatchApplyRequest(PatchApplyRequestPayload),
    PatchApplyResult(PatchApplyResultPayload),

    // Test execution (real coding validation tool)
    TestRunRequest(TestRunRequestPayload),
    TestRunResult(TestRunResultPayload),

    // Vision tools
    ScreenshotRequest(ScreenshotRequestPayload),
    ScreenshotResult(ScreenshotResultPayload),
}

// Convenience payload for the Evaluator (can be embedded in EvaluationVerdict)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationVerdictPayload {
    pub verdict_id: Uuid,
    pub session_id: Uuid,
    pub overall: String,
    pub passed_rubrics: u8,
    pub total_rubrics: u8,
    pub justifications: Vec<String>,
    pub recommended_action: String,
    pub semantic_entropy: f32,
    pub doom_loop_detected: bool,
    pub productive_death: bool,
}

// Swarm telemetry shape (per the Heavy-Tier spec)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmTelemetryPulsePayload {
    pub timestamp: String,
    pub agent_id: String,
    pub risk_score: f32,
    pub epistemic_confidence: f32,
    pub conflict_rate: f32,
    pub token_velocity: f32,
    pub gpu_util: f32,
    pub verified_count_delta: i32,
    pub authority_improvement: f32,
    pub surface_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionAttachment {
    pub name: String,
    pub mime_type: String,
    pub data_base64: String,
    pub description: String,
    pub verdict: String,                 // "APPROVED" | "REDACTED" | "BLOCKED" | "PENDING"
    pub infraction_patterns: Vec<String>,
    pub raw_data_base64: Option<String>,
    #[serde(default)]
    pub temporal_frame_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotRequestPayload {
    pub target_name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotResultPayload {
    pub attachment: VisionAttachment,
    pub error: Option<String>,
}

/// Campaign-level transactional log entry (one per Arena round + final summary).
/// This is now a first-class ACP message type so .ktrans can travel over the wire
/// exactly like SwarmTelemetryPulse or EvaluationVerdict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CampaignKtrans {
    pub tx_id: uuid::Uuid,
    pub session_id: uuid::Uuid,
    pub round: usize, // 0 = initial, 1..N = arena rounds, 999 = final
    pub timestamp: String,
    pub arena_winner: String,
    pub arena_confidence: f32,
    pub mutations_this_round: usize,
    pub verdict: serde_json::Value,
    pub leader_action: String,
    pub new_swarm_size: u32,
    pub total_mutations_so_far: usize,
    pub tx_hash: String,
    pub parent_hashes: Vec<String>,
    pub state_merkle_root: String,
    pub codebase_merkle_root: String,
    pub signature: Option<SignatureObject>,
    pub vision_attachments: Option<Vec<VisionAttachment>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CampaignKtransPayload {
    pub tx_id: uuid::Uuid,
    pub session_id: uuid::Uuid,
    pub round: usize,
    pub timestamp: String,
    pub arena_winner: String,
    pub arena_confidence: f32,
    pub mutations_this_round: usize,
    pub verdict: serde_json::Value,
    pub leader_action: String,
    pub new_swarm_size: u32,
    pub total_mutations_so_far: usize,
    pub tx_hash: String,
    pub parent_hashes: Vec<String>,
    pub state_merkle_root: String,
    pub codebase_merkle_root: String,
    pub vision_attachments: Option<Vec<VisionAttachment>>,
}

// === Coding Tool Payload Structs (Option C) ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReadRequestPayload {
    pub path: String,
    pub max_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReadResultPayload {
    pub path: String,
    pub content: String,
    pub bytes_read: u64,
    pub truncated: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellExecRequestPayload {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellExecResultPayload {
    pub command: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeEditProposalPayload {
    pub file_path: String,
    pub diff: String,
    pub description: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchApplyRequestPayload {
    pub file_path: String,
    pub patch: String,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchApplyResultPayload {
    pub file_path: String,
    pub success: bool,
    pub applied_hunks: usize,
    pub rejected_hunks: usize,
    pub new_content_preview: Option<String>,
    pub error: Option<String>,
}

// === Test Execution Payloads (next coding capability) ===
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRunRequestPayload {
    pub command: String,           // "cargo", "uv", etc.
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub timeout_ms: Option<u64>,
    pub with_coverage: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRunResultPayload {
    pub command: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    pub tests_run: u32,
    pub tests_passed: u32,
    pub tests_failed: u32,
    pub tests_ignored: u32,
    pub coverage_percent: Option<f32>,
    pub failure_summaries: Vec<String>,   // first few failing test names + messages
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
}

// =============================================================================
// ACP Framed Transport Helpers (Phase A — signed MessageEnvelope on the wire)
// =============================================================================

use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

/// Writes a payload as a signed `MessageEnvelope<AcpMessage>` using newline-delimited JSON.
/// This is the canonical ACP v1.17 wire format used by the harness for leader ↔ worker.
pub async fn write_signed_acp_envelope<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    signing_key: &SigningKey,
    payload: AcpMessage,
) -> Result<()> {
    // Sign the payload first (before moving it into the envelope)
    let signature = sign_payload(signing_key, &payload)?;

    let envelope: MessageEnvelope<AcpMessage> = MessageEnvelope {
        message_id: Uuid::new_v4(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        sender: "leader".to_string(),
        payload,
        signature,
    };

    let line = serde_json::to_string(&envelope)? + "\n";
    writer.write_all(line.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

/// Reads one newline-terminated line and deserializes it as `MessageEnvelope<AcpMessage>`.
/// The caller is responsible for calling `verify_envelope` if strict verification is desired.
pub async fn read_acp_envelope<R: AsyncBufReadExt + Unpin>(
    reader: &mut R,
) -> Result<MessageEnvelope<AcpMessage>> {
    let mut line = String::new();
    if reader.read_line(&mut line).await? == 0 {
        anyhow::bail!("EOF while reading ACP envelope");
    }
    let envelope: MessageEnvelope<AcpMessage> = serde_json::from_str(line.trim())?;
    Ok(envelope)
}

/// Convenience: read an envelope and attempt verification. Returns the inner payload if successful.
pub async fn read_and_verify_acp_envelope<R: AsyncBufReadExt + Unpin>(
    reader: &mut R,
) -> Result<(AcpMessage, bool)> {
    let envelope = read_acp_envelope(reader).await?;
    let verified = verify_envelope(&envelope).unwrap_or(false);
    Ok((envelope.payload, verified))
}
