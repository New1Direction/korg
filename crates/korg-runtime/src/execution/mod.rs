//! Thumper execution substrate — speculative DAG scheduling, self-healing
//! recovery, warm sandbox pool, and NDJSON event types.
//!
//! Folded into korg-runtime as `execution/` so these modules can stay tightly
//! coupled to the orchestration engine without forcing a public API boundary.
//! `korg_runtime::dag` re-exports the public surface for existing call sites.

pub mod dag;
pub mod events;
pub mod pool;
pub mod recovery;

pub use dag::{DagNode, ExecutionDag, ExecutionSummary, NodeStatus, SpeculativeScheduler};
pub use events::{BunEvent, BunEventOrOutcome, BunOutcome, EventLevel};
pub use recovery::{heal_node, heal_node_with_context};
