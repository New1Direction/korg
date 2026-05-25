// Re-export shim — preserves all existing `crate::dag::*` call sites.
// The real implementation lives in `crate::execution::dag` and `crate::execution::recovery`.
pub use crate::execution::dag::*;
// heal_node_with_context is NOT covered by the glob above — it lives in
// execution::recovery, not execution::dag. This line is load-bearing:
// leader.rs and workers.rs both reference it via `crate::dag::heal_node_with_context`.
pub use crate::execution::recovery::heal_node_with_context;
