//! Re-export of the standalone [`korg_ledger`] crate (the korg-ledger@v1 spec impl).
//!
//! The tamper-evident hash-chain primitives now live in their own publishable,
//! independently-auditable crate (`korg-ledger`). They are re-exported here so existing
//! `crate::ledger_chain::…` paths and the public `korg-registry` API stay unchanged —
//! single source of truth, no duplicated implementation to drift.

pub use korg_ledger::*;
