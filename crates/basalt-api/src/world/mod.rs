//! World access: block states, collision, block entities, chunk storage.
//!
//! This module is the plugin-facing entry point for all world-related
//! types. The runtime [`World`] struct, chunk storage, and persistence
//! live in `basalt-world` and are re-exported here. Pure data types
//! and helpers (collision math, block constants) progressively move
//! into `basalt-api` itself as they're decoupled from the runtime.

pub mod collision;
pub mod handle;

pub use basalt_world::block;
pub use basalt_world::block_entity;
pub use basalt_world::chunk;
pub use basalt_world::format;
pub use basalt_world::palette;
pub use basalt_world::{ChunkColumn, FlatWorldGenerator, NoiseTerrainGenerator, World};
