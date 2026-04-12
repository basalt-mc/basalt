//! Basalt world generation and chunk management.
//!
//! Provides terrain generation, chunk caching, and block storage for
//! the Minecraft server. The `World` struct is the main entry point —
//! it lazily generates and caches chunks, with optional disk persistence
//! via `basalt-storage`.

pub mod block;
pub mod chunk;
pub mod format;
mod generator;
mod noise_gen;
pub mod palette;

pub use chunk::ChunkColumn;
pub use generator::FlatWorldGenerator;
pub use noise_gen::NoiseTerrainGenerator;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use basalt_storage::RegionStorage;

/// How terrain is generated for new chunks.
enum Generator {
    /// Superflat world: bedrock + dirt + grass.
    Flat(FlatWorldGenerator),
    /// Noise-based terrain with hills, water, beaches.
    Noise(Box<NoiseTerrainGenerator>),
}

/// A Minecraft world with lazy chunk generation, in-memory caching,
/// and optional disk persistence.
///
/// Load order: memory cache → disk → generate.
/// Generated chunks are saved to disk (if storage is configured)
/// and cached in memory as encoded packets.
pub struct World {
    /// Cached encoded packets for fast repeated access.
    packets:
        Mutex<HashMap<(i32, i32), basalt_protocol::packets::play::world::ClientboundPlayMapChunk>>,
    /// The terrain generator used for new chunks.
    generator: Generator,
    /// The Y coordinate where players spawn.
    spawn_y: i32,
    /// Optional disk storage for chunk persistence.
    storage: Option<RegionStorage>,
}

impl World {
    /// Creates a new world with noise-based terrain and disk persistence.
    ///
    /// Chunks are saved to `save_dir/regions/` as BSR region files.
    pub fn new(seed: u32, save_dir: impl Into<PathBuf>) -> Self {
        let save_path = save_dir.into();
        let storage = RegionStorage::new(save_path.join("regions")).ok();
        Self {
            packets: Mutex::new(HashMap::new()),
            generator: Generator::Noise(Box::new(NoiseTerrainGenerator::new(seed))),
            spawn_y: NoiseTerrainGenerator::SPAWN_Y,
            storage,
        }
    }

    /// Creates a new world with noise-based terrain, no persistence.
    pub fn new_memory(seed: u32) -> Self {
        Self {
            packets: Mutex::new(HashMap::new()),
            generator: Generator::Noise(Box::new(NoiseTerrainGenerator::new(seed))),
            spawn_y: NoiseTerrainGenerator::SPAWN_Y,
            storage: None,
        }
    }

    /// Creates a new flat world (superflat), no persistence.
    pub fn flat() -> Self {
        Self {
            packets: Mutex::new(HashMap::new()),
            generator: Generator::Flat(FlatWorldGenerator),
            spawn_y: FlatWorldGenerator::SPAWN_Y,
            storage: None,
        }
    }

    /// The Y coordinate where players should spawn.
    pub fn spawn_y(&self) -> f64 {
        self.spawn_y as f64
    }

    /// Returns a protocol packet for the chunk at (cx, cz).
    ///
    /// Load order: memory cache → disk → generate.
    /// Newly generated chunks are saved to disk and cached in memory.
    pub fn get_chunk_packet(
        &self,
        cx: i32,
        cz: i32,
    ) -> basalt_protocol::packets::play::world::ClientboundPlayMapChunk {
        let mut cache = self.packets.lock().unwrap();
        cache
            .entry((cx, cz))
            .or_insert_with(|| {
                // Try loading from disk first
                if let Some(storage) = &self.storage
                    && let Ok(Some(data)) = storage.load_raw(cx, cz)
                    && let Some(col) = format::deserialize_chunk(&data, cx, cz)
                {
                    return col.to_packet();
                }

                // Generate new chunk
                let mut col = ChunkColumn::new(cx, cz);
                match &self.generator {
                    Generator::Flat(g) => g.generate(&mut col),
                    Generator::Noise(g) => g.generate(&mut col),
                }

                // Save to disk
                if let Some(storage) = &self.storage {
                    let data = format::serialize_chunk(&col);
                    let _ = storage.save_raw(cx, cz, &data);
                }

                col.to_packet()
            })
            .clone()
    }

    /// Returns true if the chunk at (cx, cz) is in the memory cache.
    pub fn is_chunk_loaded(&self, cx: i32, cz: i32) -> bool {
        self.packets.lock().unwrap().contains_key(&(cx, cz))
    }

    /// Returns the number of chunks currently in the memory cache.
    pub fn chunk_count(&self) -> usize {
        self.packets.lock().unwrap().len()
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new_memory(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_generates_on_first_access() {
        let world = World::new_memory(42);
        assert!(!world.is_chunk_loaded(0, 0));

        let packet = world.get_chunk_packet(0, 0);
        assert_eq!(packet.x, 0);
        assert_eq!(packet.z, 0);
        assert!(world.is_chunk_loaded(0, 0));
    }

    #[test]
    fn world_caches_chunks() {
        let world = World::new_memory(42);
        world.get_chunk_packet(0, 0);
        world.get_chunk_packet(0, 0);
        assert_eq!(world.chunk_count(), 1);
    }

    #[test]
    fn world_generates_different_coords() {
        let world = World::new_memory(42);
        world.get_chunk_packet(0, 0);
        world.get_chunk_packet(1, 0);
        world.get_chunk_packet(0, 1);
        assert_eq!(world.chunk_count(), 3);
    }

    #[test]
    fn spawn_y_is_above_ground() {
        let world = World::new_memory(42);
        assert_eq!(world.spawn_y(), NoiseTerrainGenerator::SPAWN_Y as f64);
    }

    #[test]
    fn world_with_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new(42, dir.path());

        // Generate and save
        world.get_chunk_packet(0, 0);
        world.get_chunk_packet(1, 1);

        // Create a new world from the same directory — should load from disk
        let world2 = World::new(42, dir.path());
        assert!(!world2.is_chunk_loaded(0, 0)); // not in memory yet
        world2.get_chunk_packet(0, 0); // loads from disk
        assert!(world2.is_chunk_loaded(0, 0));
    }
}
