// Re-export shim — preserves all existing `crate::dag::*` call sites.
// The real implementation lives in `crate::execution`.
pub use crate::execution::dag::*;
pub use crate::execution::recovery::heal_node_with_context;
