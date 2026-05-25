// Orchestration cluster — ships as one crate the same way bevy_ecs ships as
// one crate. These modules form a single connected component; splitting them
// further would force every internal interface to become a public API.
//
// Architecture/Overview.md §korg-runtime documents the design rationale.
// Migration step 6.

#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]
#![allow(unused_assignments)]

pub mod tui_bridge;
pub mod recovery;
pub mod acp;
pub mod agent;
pub mod arena;
pub mod blackboard;
pub mod campaign;
pub mod code_indexer;
pub mod code_intel;
pub mod dag;
pub mod evaluator;
pub mod harness;
pub mod leader;
pub mod personas;
pub mod provenance;
pub mod runtime;
pub mod session;
pub mod skills;
pub mod tools;
pub mod vision_policy;
pub mod workers;
pub mod workspace;
