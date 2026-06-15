//! Per-persona permission policy (SP2 Slice 3).
//!
//! Personas have real, role-shaped capabilities instead of a single hardcoded
//! `fs:write:worktree` grant for everyone:
//!
//! - **Benjamin / Lucas** implement and synthesize patches → they may write the
//!   worktree (`fs:write:worktree`).
//! - **Harper / Captain / Evaluator** plan, research, and evaluate → they are
//!   read-only (`fs:read`). A read-only persona that nonetheless *emits*
//!   mutations must NOT mutate the worktree; it is analyzed (numstat / cargo
//!   check on the existing tree) and recorded as `files_changed = 0` honestly.
//!
//! The capability list is the flat `Vec<String>` shape already carried by
//! `RouteWork.permissions`, so this drops in without an ACP schema change.

/// Capability granting write access to the worker's isolated worktree.
pub const CAP_FS_WRITE_WORKTREE: &str = "fs:write:worktree";

/// Capability granting read-only access (analyze, never mutate).
pub const CAP_FS_READ: &str = "fs:read";

/// Resolve the capability list for a persona by name.
///
/// Matching is case-insensitive and tolerant of the decorated worker id form
/// (e.g. `"benjamin-019ec8…"`), so callers can pass either the bare persona
/// name (`spec.persona`) or a worker id without surprises.
///
/// Implementers (write the worktree): benjamin, lucas.
/// Read-only (analyze only): harper, captain, evaluator.
/// Unknown personas default to read-only — the safe, least-privilege choice.
pub fn permissions_for(persona: &str) -> Vec<String> {
    let p = persona.to_lowercase();
    if p.contains("benjamin") || p.contains("lucas") {
        vec![CAP_FS_WRITE_WORKTREE.to_string()]
    } else {
        // harper / captain / evaluator — and any unrecognized persona — are
        // read-only by default (least privilege).
        vec![CAP_FS_READ.to_string()]
    }
}

/// The apply gate: may this permission set mutate the worktree?
///
/// `true` iff the capability list contains `fs:write:worktree`. A persona
/// without that capability is analyze-only — its emitted mutations are observed
/// but never written.
pub fn may_write(permissions: &[String]) -> bool {
    permissions.iter().any(|c| c == CAP_FS_WRITE_WORKTREE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benjamin_has_write_capability() {
        let perms = permissions_for("benjamin");
        assert!(
            may_write(&perms),
            "benjamin (implementer) must be able to write the worktree"
        );
        assert_eq!(perms, vec![CAP_FS_WRITE_WORKTREE.to_string()]);
    }

    #[test]
    fn lucas_has_write_capability() {
        // Lucas synthesizes and applies the synthesized patch → write.
        assert!(may_write(&permissions_for("lucas")));
    }

    #[test]
    fn harper_is_read_only() {
        let perms = permissions_for("harper");
        assert!(
            !may_write(&perms),
            "harper (researcher) must be read-only — analyze, never mutate"
        );
        assert_eq!(perms, vec![CAP_FS_READ.to_string()]);
    }

    #[test]
    fn captain_is_read_only() {
        assert!(!may_write(&permissions_for("captain")));
    }

    #[test]
    fn evaluator_is_read_only() {
        assert!(!may_write(&permissions_for("evaluator")));
    }

    #[test]
    fn matching_is_case_insensitive_and_tolerates_worker_id_form() {
        // Bare name, capitalized name, and decorated worker-id form all resolve.
        assert!(may_write(&permissions_for("Benjamin")));
        assert!(may_write(&permissions_for("benjamin-019ec826-9422-7cc2")));
        assert!(!may_write(&permissions_for("Harper")));
        assert!(!may_write(&permissions_for("harper-019ec826-9422-7cc2")));
    }

    #[test]
    fn unknown_persona_defaults_to_read_only() {
        // Least privilege: an unrecognized persona must not get write.
        assert!(!may_write(&permissions_for("some-unknown-persona")));
    }

    #[test]
    fn may_write_is_capability_driven_not_persona_driven() {
        // The gate keys on the capability, so an explicit write grant works
        // regardless of how it was derived.
        assert!(may_write(&[CAP_FS_WRITE_WORKTREE.to_string()]));
        assert!(!may_write(&[CAP_FS_READ.to_string()]));
        assert!(!may_write(&[]));
    }
}
