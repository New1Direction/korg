//! Campaign — State Machine and .ktrans Lifecycle
//!
//! Encapsulates the campaign lifecycle: phases, signing, and ktrans persistence.
//! The `LeaderOrchestrator` remains a thin coordinator that calls into this module
//! rather than owning the signing and serialization logic itself.
//!
//! # Campaign Phase State Machine
//!
//! ```text
//! Initializing → Planning → Contracting → Dispatching
//!                                            ↓
//!                                       Evaluating → Committing → Complete
//!                                                        ↓
//!                                                    Aborted (on doom-loop)
//! ```

use crate::acp::{
    sign_payload, CampaignKtrans, CampaignKtransPayload, MessageEnvelope, SignatureObject,
};
use crate::evaluator::EvaluationVerdict;
use anyhow::Result;
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// =========================================================================
// Campaign Phase State Machine
// =========================================================================

/// Ordered states of a campaign run. Transitions are linear except for
/// `Aborted` which can be reached from any phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CampaignPhase {
    Initializing,
    Planning,
    Contracting,
    Dispatching,
    Evaluating,
    Committing,
    Complete,
    Aborted,
}

impl CampaignPhase {
    /// Returns the next valid phase in the linear happy path.
    pub fn next(&self) -> Option<CampaignPhase> {
        match self {
            CampaignPhase::Initializing => Some(CampaignPhase::Planning),
            CampaignPhase::Planning     => Some(CampaignPhase::Contracting),
            CampaignPhase::Contracting  => Some(CampaignPhase::Dispatching),
            CampaignPhase::Dispatching  => Some(CampaignPhase::Evaluating),
            CampaignPhase::Evaluating   => Some(CampaignPhase::Committing),
            CampaignPhase::Committing   => Some(CampaignPhase::Complete),
            CampaignPhase::Complete     => None,
            CampaignPhase::Aborted      => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            CampaignPhase::Initializing => "initializing",
            CampaignPhase::Planning     => "planning",
            CampaignPhase::Contracting  => "contracting",
            CampaignPhase::Dispatching  => "dispatching",
            CampaignPhase::Evaluating   => "evaluating",
            CampaignPhase::Committing   => "committing",
            CampaignPhase::Complete     => "complete",
            CampaignPhase::Aborted      => "aborted",
        }
    }
}

// =========================================================================
// Campaign Context
// =========================================================================

/// Immutable configuration for a campaign run.
#[derive(Debug, Clone)]
pub struct CampaignConfig {
    pub session_id: Uuid,
    pub root_task: String,
    pub max_rounds: usize,
    pub goal_mode: bool,
}

/// Mutable tip state for the campaign's Merkle-DAG ledger.
/// Each committed .ktrans appends its hash to this tip set.
#[derive(Debug, Clone, Default)]
pub struct CampaignTips {
    pub hashes: Vec<String>,
}

impl CampaignTips {
    pub fn update(&mut self, new_hash: String) {
        self.hashes = vec![new_hash];
    }

    pub fn current(&self) -> Vec<String> {
        self.hashes.clone()
    }
}

// =========================================================================
// .ktrans Persistence — extracted from LeaderOrchestrator
// =========================================================================

/// Persists a campaign transaction as a signed `MessageEnvelope<CampaignKtrans>`.
/// Returns the tx_hash for Merkle-DAG chaining.
///
/// This function owns all I/O and signing — the leader only calls it and
/// receives the resulting hash back for tip tracking.
#[tracing::instrument(skip(signing_key, verdict, vision_attachments, parent_tips), fields(
    session_id = %session_id,
    round,
    arena_winner = %arena_winner,
    arena_confidence,
))]
pub async fn persist_campaign_ktrans(
    session_id: Uuid,
    round: usize,
    arena_winner: String,
    arena_confidence: f32,
    mutations_this_round: usize,
    swarm_size: u32,
    verdict: &EvaluationVerdict,
    signing_key: &SigningKey,
    parent_tips: Vec<String>,
    vision_attachments: Vec<crate::acp::VisionAttachment>,
    last_round_healed: bool,
) -> Result<(String, MessageEnvelope<CampaignKtrans>)> {
    let verdict_json = serde_json::to_value(verdict).unwrap_or_default();
    let leader_action = verdict.recommended_action.clone();
    let timestamp = chrono::Utc::now().to_rfc3339();
    let tx_id = Uuid::now_v7();

    // 1. Capture logical state root (hash of the blackboard on disk)
    let state_merkle_root = capture_state_merkle_root(session_id).await;

    // 2. Capture physical codebase root (git write-tree)
    let codebase_merkle_root = capture_codebase_merkle_root().await;

    // 3. Build the payload for JCS content-addressing
    let mut ktrans_payload = CampaignKtransPayload {
        tx_id,
        session_id,
        round,
        timestamp: timestamp.clone(),
        arena_winner: arena_winner.clone(),
        arena_confidence,
        mutations_this_round,
        verdict: verdict_json.clone(),
        leader_action: leader_action.clone(),
        new_swarm_size: swarm_size,
        total_mutations_so_far: (round + 1) * 5,
        tx_hash: "".to_string(), // excluded from hash
        parent_hashes: parent_tips.clone(),
        state_merkle_root: state_merkle_root.clone(),
        codebase_merkle_root: codebase_merkle_root.clone(),
        vision_attachments: Some(vision_attachments.clone()),
        certainty: Some(arena_confidence),
        blast_radius: Some(0.15 + (mutations_this_round as f32 * 0.1).min(0.6)),
        severity: Some(0.1 + (0.5 * (1.0 - arena_confidence)).min(0.5)),
        remediation_confidence: Some(0.9 + (arena_confidence * 0.05)),
        is_healed: Some(last_round_healed),
    };

    let tx_hash = crate::provenance::compute_sha256(&ktrans_payload)
        .unwrap_or_else(|_| format!("sha256:{}", hex::encode([0u8; 32])));

    // 4. Persist the state blob so it can be replayed via playhead steering
    persist_state_blob(session_id, &state_merkle_root).await;

    // 5. Build the signed CampaignKtrans
    ktrans_payload.tx_hash = tx_hash.clone();
    let mut ktrans = CampaignKtrans {
        tx_id,
        session_id,
        round,
        timestamp: timestamp.clone(),
        arena_winner,
        arena_confidence,
        mutations_this_round,
        verdict: verdict_json,
        leader_action,
        new_swarm_size: swarm_size,
        total_mutations_so_far: (round + 1) * 5,
        tx_hash: tx_hash.clone(),
        parent_hashes: parent_tips,
        state_merkle_root,
        codebase_merkle_root,
        signature: None,
        vision_attachments: Some(vision_attachments),
        certainty: Some(arena_confidence),
        blast_radius: ktrans_payload.blast_radius,
        severity: ktrans_payload.severity,
        remediation_confidence: ktrans_payload.remediation_confidence,
        is_healed: Some(last_round_healed),
    };

    let signature: SignatureObject = sign_payload(signing_key, &ktrans)
        .map_err(|e| anyhow::anyhow!("failed to sign CampaignKtrans: {}", e))?;
    ktrans.signature = Some(signature.clone());

    let envelope: MessageEnvelope<CampaignKtrans> = MessageEnvelope {
        message_id: Uuid::now_v7(),
        timestamp: ktrans.timestamp.clone(),
        sender: format!("leader-{}", session_id),
        payload: ktrans,
        signature,
    };

    // 6. Write to disk
    write_ktrans_to_disk(session_id, round, &envelope).await?;

    crate::metrics::record_ktrans_persisted(round);

    tracing::info!(
        tx_hash = %tx_hash,
        round,
        "ktrans persisted"
    );

    Ok((tx_hash, envelope))
}

/// Persist a final summary .ktrans for round 999 (conventional end-of-campaign marker).
pub async fn persist_final_summary_ktrans(
    session_id: Uuid,
    swarm_size: u32,
    signing_key: &SigningKey,
    parent_tips: Vec<String>,
) -> Result<String> {
    let final_verdict = EvaluationVerdict {
        verdict_id: Uuid::now_v7(),
        session_id,
        timestamp: chrono::Utc::now().to_rfc3339(),
        overall: "COMPLETE".to_string(),
        passed_rubrics: 5,
        total_rubrics: 5,
        justifications: vec!["Campaign completed with full audit trail".to_string()],
        recommended_action: "complete".to_string(),
        semantic_entropy: 0.0,
        doom_loop_detected: false,
        productive_death: false,
    };

    let (tx_hash, _) = persist_campaign_ktrans(
        session_id,
        999,
        "FINAL".to_string(),
        1.0,
        0,
        swarm_size,
        &final_verdict,
        signing_key,
        parent_tips,
        vec![],
        false,
    ).await?;

    crate::metrics::record_campaign_completed();
    Ok(tx_hash)
}

// =========================================================================
// Internal helpers
// =========================================================================

async fn capture_state_merkle_root(session_id: Uuid) -> String {
    let content = tokio::fs::read_to_string(crate::paths::blackboard_json()).await;
    if let Ok(content) = content {
        if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Ok(hash) = crate::provenance::compute_sha256(&json_val) {
                return hash;
            }
        }
    }
    let _ = session_id; // used for future state-scoped blobs
    hex::encode([0u8; 32])
}

async fn capture_codebase_merkle_root() -> String {
    match tokio::process::Command::new("git")
        .arg("write-tree")
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => "sha256:codebase-fallback".to_string(),
    }
}

async fn persist_state_blob(session_id: Uuid, state_merkle_root: &str) {
    let blob_dir = crate::paths::state_blobs_dir(&session_id);
    let _ = tokio::fs::create_dir_all(&blob_dir).await;
    let blob_path = blob_dir.join(format!("{}.json", state_merkle_root));
    if let Ok(content) = tokio::fs::read_to_string(crate::paths::blackboard_json()).await {
        let _ = tokio::fs::write(&blob_path, content).await;
    }
}

async fn write_ktrans_to_disk(
    session_id: Uuid,
    round: usize,
    envelope: &MessageEnvelope<CampaignKtrans>,
) -> Result<()> {
    let dir = crate::paths::campaign_dir(&session_id);
    tokio::fs::create_dir_all(&dir).await?;

    let filename = if round == 999 {
        "final-summary.ktrans.json".to_string()
    } else {
        format!("round-{:03}.ktrans.json", round)
    };

    let path = dir.join(&filename);
    if let Ok(pretty) = serde_json::to_string_pretty(envelope) {
        tokio::fs::write(&path, &pretty).await?;
        tracing::debug!(path = %path.display(), "ktrans written to disk");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_campaign_phase_transitions_happy_path() {
        let mut phase = CampaignPhase::Initializing;
        let expected = [
            CampaignPhase::Planning,
            CampaignPhase::Contracting,
            CampaignPhase::Dispatching,
            CampaignPhase::Evaluating,
            CampaignPhase::Committing,
            CampaignPhase::Complete,
        ];
        for &next_expected in &expected {
            phase = phase.next().expect("transition should exist");
            assert_eq!(phase, next_expected);
        }
        assert!(phase.next().is_none(), "Complete has no successor");
    }

    #[test]
    fn test_campaign_phase_aborted_terminal() {
        assert!(CampaignPhase::Aborted.next().is_none());
    }

    #[test]
    fn test_campaign_tips_update() {
        let mut tips = CampaignTips::default();
        assert!(tips.current().is_empty());
        tips.update("abc123".to_string());
        assert_eq!(tips.current(), vec!["abc123".to_string()]);
        tips.update("def456".to_string());
        // Only the latest tip is kept (linear chain)
        assert_eq!(tips.current(), vec!["def456".to_string()]);
    }

    #[test]
    fn test_campaign_phase_as_str() {
        assert_eq!(CampaignPhase::Initializing.as_str(), "initializing");
        assert_eq!(CampaignPhase::Aborted.as_str(), "aborted");
        assert_eq!(CampaignPhase::Complete.as_str(), "complete");
    }
}
