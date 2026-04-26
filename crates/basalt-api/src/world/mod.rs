//! World access: block states, collision, block entities, chunk storage.
//!
//! This module is the plugin-facing entry point for all world-related
//! types. Block constants, block entity types, collision math, and the
//! [`WorldHandle`](handle::WorldHandle) trait live here. The runtime
//! `World` struct, chunk storage, and persistence live in `basalt-world`
//! (which depends on this crate for the shared types).

pub mod block;
pub mod block_entity;
pub mod collision;
pub mod handle;
