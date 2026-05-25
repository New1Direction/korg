//! Foundation crate for the korg cognitive runtime.
//!
//! # Inclusion criterion
//!
//! A module belongs in `korg-core` if and only if it meets **both** conditions:
//!   1. Zero `use crate::` (now `use korg_*::`) internal dependencies — it could
//!      compile as a standalone crate with only third-party deps.
//!   2. Consumed by three or more other korg crates.
//!
//! If a module is isolated but consumed by only one crate, it belongs inside that
//! crate, not here. If a module has internal deps, it belongs in the lowest crate
//! in the dep graph that satisfies those deps.
//!
//! **What does NOT belong here:**
//! - `provenance.rs` — zero external deps but depends on `acp::canonicalize` and
//!   `acp` message types; lives in `korg-runtime` until those can be lifted.
//! - Any module that references `korg-registry`, `korg-runtime`, or higher crates.

pub mod adapter;
pub mod event;
pub mod metrics;
pub mod paths;
pub mod subscription;
pub mod telemetry;

pub use adapter::Adapter;
pub use event::{ContentRef, NormalizedEvent};
pub use subscription::SubscriptionTier;
