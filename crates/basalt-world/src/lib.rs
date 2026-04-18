//! Basalt world generation and chunk management.
//!
//! Provides terrain generation, chunk caching with LRU eviction, and
//! block storage for the Minecraft server. The `World` struct is the
//! main entry point — it lazily generates and caches chunks, with
//! optional disk persistence via `basalt-storage`.
//!
//! Uses `DashMap` for concurrent per-chunk access instead of a single
//! global `Mutex`. Each chunk is independently lockable, so players
//! streaming different chunks don't block each other.

pub mod block;
pub mod block_entity;
pub mod chunk;
pub mod collision;
pub mod format;
mod generator;
mod noise_gen;
pub mod palette;

pub use chunk::ChunkColumn;
pub use generator::FlatWorldGenerator;
pub use noise_gen::NoiseTerrainGenerator;

mod world;

pub use world::World;
