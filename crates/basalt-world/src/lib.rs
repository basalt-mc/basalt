//! Basalt world generation and chunk management.
//!
//! Provides terrain generation, chunk caching, and block storage for
//! the Minecraft server. The `World` struct is the main entry point —
//! it lazily generates and caches chunks as they are requested.
//!
//! # Architecture
//!
//! ```text
//! World (cache + generator)
//!   └── ChunkColumn (24 sections)
//!         └── PalettedContainer (16×16×16 blocks)
//!               └── Block state IDs
//! ```

pub mod block;
mod chunk;
mod generator;
mod noise_gen;
mod palette;

pub use chunk::ChunkColumn;
pub use generator::FlatWorldGenerator;
pub use noise_gen::NoiseTerrainGenerator;

use std::collections::HashMap;
use std::sync::Mutex;

/// How terrain is generated for new chunks.
enum Generator {
    /// Superflat world: bedrock + dirt + grass.
    Flat(FlatWorldGenerator),
    /// Noise-based terrain with hills, water, beaches.
    Noise(Box<NoiseTerrainGenerator>),
}

/// A Minecraft world with lazy chunk generation and in-memory caching.
///
/// Chunks are generated on first access using the configured generator
/// and stored in a `HashMap` keyed by (chunk_x, chunk_z). All generated
/// chunks persist in memory for the lifetime of the world.
///
/// Thread-safe: the chunk cache is behind a `Mutex` so multiple player
/// tasks can request chunks concurrently.
pub struct World {
    /// In-memory chunk cache, keyed by (chunk_x, chunk_z).
    chunks: Mutex<HashMap<(i32, i32), ChunkColumn>>,
    /// The terrain generator used for new chunks.
    generator: Generator,
    /// The Y coordinate where players spawn.
    spawn_y: i32,
}

impl World {
    /// Creates a new world with noise-based terrain generation.
    ///
    /// The seed determines the terrain shape — same seed produces
    /// the same world every time. No chunks are generated until
    /// they are requested.
    pub fn new(seed: u32) -> Self {
        Self {
            chunks: Mutex::new(HashMap::new()),
            generator: Generator::Noise(Box::new(NoiseTerrainGenerator::new(seed))),
            spawn_y: NoiseTerrainGenerator::SPAWN_Y,
        }
    }

    /// Creates a new flat world (superflat).
    pub fn flat() -> Self {
        Self {
            chunks: Mutex::new(HashMap::new()),
            generator: Generator::Flat(FlatWorldGenerator),
            spawn_y: FlatWorldGenerator::SPAWN_Y,
        }
    }

    /// The Y coordinate where players should spawn.
    pub fn spawn_y(&self) -> f64 {
        self.spawn_y as f64
    }

    /// Returns a protocol packet for the chunk at (cx, cz).
    ///
    /// If the chunk is not yet generated, generates it first and
    /// stores it in the cache. Subsequent calls for the same
    /// coordinates return the cached version.
    pub fn get_chunk_packet(
        &self,
        cx: i32,
        cz: i32,
    ) -> basalt_protocol::packets::play::world::ClientboundPlayMapChunk {
        let mut cache = self.chunks.lock().unwrap();
        let chunk = cache.entry((cx, cz)).or_insert_with(|| {
            let mut col = ChunkColumn::new(cx, cz);
            match &self.generator {
                Generator::Flat(g) => g.generate(&mut col),
                Generator::Noise(g) => g.generate(&mut col),
            }
            col
        });
        chunk.to_packet()
    }

    /// Returns true if the chunk at (cx, cz) has been generated.
    pub fn is_chunk_loaded(&self, cx: i32, cz: i32) -> bool {
        self.chunks.lock().unwrap().contains_key(&(cx, cz))
    }

    /// Returns the number of chunks currently cached.
    pub fn chunk_count(&self) -> usize {
        self.chunks.lock().unwrap().len()
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_generates_on_first_access() {
        let world = World::new(42);
        assert!(!world.is_chunk_loaded(0, 0));

        let packet = world.get_chunk_packet(0, 0);
        assert_eq!(packet.x, 0);
        assert_eq!(packet.z, 0);
        assert!(world.is_chunk_loaded(0, 0));
    }

    #[test]
    fn world_caches_chunks() {
        let world = World::new(42);
        world.get_chunk_packet(0, 0);
        world.get_chunk_packet(0, 0); // should not regenerate
        assert_eq!(world.chunk_count(), 1);
    }

    #[test]
    fn world_generates_different_coords() {
        let world = World::new(42);
        world.get_chunk_packet(0, 0);
        world.get_chunk_packet(1, 0);
        world.get_chunk_packet(0, 1);
        assert_eq!(world.chunk_count(), 3);
    }

    #[test]
    fn spawn_y_is_above_ground() {
        let world = World::new(42);
        assert_eq!(world.spawn_y(), NoiseTerrainGenerator::SPAWN_Y as f64);
    }
}
