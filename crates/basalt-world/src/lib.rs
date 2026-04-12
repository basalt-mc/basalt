//! Basalt world generation and chunk management.
//!
//! Provides terrain generation, chunk caching, and block storage for
//! the Minecraft server. The `World` struct is the main entry point —
//! it lazily generates and caches chunks, with optional disk persistence
//! via `basalt-storage`.
//!
//! Blocks can be read and modified via `get_block` and `set_block`.
//! Modifications invalidate the packet cache for the affected chunk
//! and persist to disk if storage is configured.

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

/// Chunk data and packet cache behind a single lock.
///
/// Keeping both in one struct avoids holding two separate locks and
/// prevents deadlock scenarios. The packet cache is a lazy derivative
/// of the chunk data — invalidated when a block changes, rebuilt on
/// next access.
struct ChunkStore {
    /// Loaded chunk columns, mutable for block modifications.
    chunks: HashMap<(i32, i32), ChunkColumn>,
    /// Cached encoded packets. Invalidated when a chunk is modified.
    packets: HashMap<(i32, i32), basalt_protocol::packets::play::world::ClientboundPlayMapChunk>,
}

/// A Minecraft world with lazy chunk generation, in-memory caching,
/// and optional disk persistence.
///
/// Load order: memory cache → disk → generate.
/// Generated chunks are saved to disk (if storage is configured)
/// and cached in memory. Blocks can be read and modified at any time;
/// modifications invalidate the packet cache and persist to disk.
pub struct World {
    /// Combined chunk and packet storage behind a single lock.
    store: Mutex<ChunkStore>,
    /// The terrain generator used for new chunks.
    generator: Generator,
    /// The Y coordinate where players spawn.
    spawn_y: i32,
    /// Optional disk storage for chunk persistence.
    storage: Option<RegionStorage>,
}

/// Ensures a chunk is loaded in the store, generating or loading from
/// disk if necessary.
///
/// This is a free function to avoid borrow conflicts — it only borrows
/// the fields it needs (`store` contents, `generator`, `storage`) without
/// holding `&self`.
fn ensure_chunk_loaded(
    store: &mut ChunkStore,
    cx: i32,
    cz: i32,
    generator: &Generator,
    storage: &Option<RegionStorage>,
) {
    if store.chunks.contains_key(&(cx, cz)) {
        return;
    }

    // Try loading from disk
    if let Some(s) = storage
        && let Ok(Some(data)) = s.load_raw(cx, cz)
        && let Some(col) = format::deserialize_chunk(&data, cx, cz)
    {
        store.chunks.insert((cx, cz), col);
        return;
    }

    // Generate new chunk
    let mut col = ChunkColumn::new(cx, cz);
    match generator {
        Generator::Flat(g) => g.generate(&mut col),
        Generator::Noise(g) => g.generate(&mut col),
    }

    // Save to disk
    if let Some(s) = storage {
        let data = format::serialize_chunk(&col);
        let _ = s.save_raw(cx, cz, &data);
    }

    store.chunks.insert((cx, cz), col);
}

impl World {
    /// Creates a new world with noise-based terrain and disk persistence.
    ///
    /// Chunks are saved to `save_dir/regions/` as BSR region files.
    pub fn new(seed: u32, save_dir: impl Into<PathBuf>) -> Self {
        let save_path = save_dir.into();
        let storage = RegionStorage::new(save_path.join("regions")).ok();
        Self {
            store: Mutex::new(ChunkStore {
                chunks: HashMap::new(),
                packets: HashMap::new(),
            }),
            generator: Generator::Noise(Box::new(NoiseTerrainGenerator::new(seed))),
            spawn_y: NoiseTerrainGenerator::SPAWN_Y,
            storage,
        }
    }

    /// Creates a new world with noise-based terrain, no persistence.
    pub fn new_memory(seed: u32) -> Self {
        Self {
            store: Mutex::new(ChunkStore {
                chunks: HashMap::new(),
                packets: HashMap::new(),
            }),
            generator: Generator::Noise(Box::new(NoiseTerrainGenerator::new(seed))),
            spawn_y: NoiseTerrainGenerator::SPAWN_Y,
            storage: None,
        }
    }

    /// Creates a new flat world (superflat), no persistence.
    pub fn flat() -> Self {
        Self {
            store: Mutex::new(ChunkStore {
                chunks: HashMap::new(),
                packets: HashMap::new(),
            }),
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
    /// Load order: packet cache → encode from chunk → disk → generate.
    /// Newly generated chunks are saved to disk, stored in memory,
    /// and their encoded packets are cached.
    pub fn get_chunk_packet(
        &self,
        cx: i32,
        cz: i32,
    ) -> basalt_protocol::packets::play::world::ClientboundPlayMapChunk {
        let mut store = self.store.lock().unwrap();

        // Return cached packet if available
        if let Some(packet) = store.packets.get(&(cx, cz)) {
            return packet.clone();
        }

        // Ensure chunk data is loaded
        ensure_chunk_loaded(&mut store, cx, cz, &self.generator, &self.storage);

        // Encode and cache
        let packet = store.chunks[&(cx, cz)].to_packet();
        store.packets.insert((cx, cz), packet.clone());
        packet
    }

    /// Sets a block at absolute world coordinates.
    ///
    /// Loads the chunk if it isn't already in memory. Invalidates the
    /// packet cache for the affected chunk so the next `get_chunk_packet`
    /// call re-encodes it. Does NOT persist to disk — call
    /// `persist_chunk()` separately (the `StorageHandler` does this).
    pub fn set_block(&self, x: i32, y: i32, z: i32, state: u16) {
        let cx = x >> 4;
        let cz = z >> 4;
        let local_x = x.rem_euclid(16) as usize;
        let local_z = z.rem_euclid(16) as usize;

        let mut store = self.store.lock().unwrap();
        ensure_chunk_loaded(&mut store, cx, cz, &self.generator, &self.storage);

        store
            .chunks
            .get_mut(&(cx, cz))
            .unwrap()
            .set_block(local_x, y, local_z, state);

        // Invalidate cached packet
        store.packets.remove(&(cx, cz));
    }

    /// Persists a chunk to disk via the storage backend.
    ///
    /// Serializes the chunk at (cx, cz) and writes it to the BSR
    /// region file. No-op if storage is not configured or the chunk
    /// is not loaded. Called by the `StorageHandler` after block changes.
    pub fn persist_chunk(&self, cx: i32, cz: i32) {
        let store = self.store.lock().unwrap();
        if let Some(s) = &self.storage
            && let Some(chunk) = store.chunks.get(&(cx, cz))
        {
            let data = format::serialize_chunk(chunk);
            let _ = s.save_raw(cx, cz, &data);
        }
    }

    /// Gets a block at absolute world coordinates.
    ///
    /// Loads the chunk if it isn't already in memory.
    pub fn get_block(&self, x: i32, y: i32, z: i32) -> u16 {
        let cx = x >> 4;
        let cz = z >> 4;
        let local_x = x.rem_euclid(16) as usize;
        let local_z = z.rem_euclid(16) as usize;

        let mut store = self.store.lock().unwrap();
        ensure_chunk_loaded(&mut store, cx, cz, &self.generator, &self.storage);

        store.chunks[&(cx, cz)].get_block(local_x, y, local_z)
    }

    /// Returns true if the chunk at (cx, cz) is in the memory cache.
    pub fn is_chunk_loaded(&self, cx: i32, cz: i32) -> bool {
        self.store.lock().unwrap().chunks.contains_key(&(cx, cz))
    }

    /// Returns the number of chunks currently in the memory cache.
    pub fn chunk_count(&self) -> usize {
        self.store.lock().unwrap().chunks.len()
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

    #[test]
    fn set_block_modifies_chunk() {
        let world = World::new_memory(42);
        // Load chunk first
        world.get_chunk_packet(0, 0);
        // Modify a block
        world.set_block(5, 64, 3, block::STONE);
        assert_eq!(world.get_block(5, 64, 3), block::STONE);
    }

    #[test]
    fn set_block_invalidates_packet_cache() {
        let world = World::new_memory(42);
        let packet1 = world.get_chunk_packet(0, 0);
        world.set_block(0, 64, 0, block::STONE);
        let packet2 = world.get_chunk_packet(0, 0);
        // The re-encoded packet should reflect the new block
        assert_ne!(packet1.chunk_data, packet2.chunk_data);
    }

    #[test]
    fn set_block_generates_chunk_if_needed() {
        let world = World::new_memory(42);
        assert!(!world.is_chunk_loaded(5, 5));
        world.set_block(80, 64, 80, block::STONE); // chunk (5, 5)
        assert!(world.is_chunk_loaded(5, 5));
        assert_eq!(world.get_block(80, 64, 80), block::STONE);
    }

    #[test]
    fn get_block_reads_generated_terrain() {
        let world = World::flat();
        // Flat world: bedrock at y=-64
        assert_eq!(world.get_block(0, -64, 0), block::BEDROCK);
        assert_eq!(world.get_block(0, -60, 0), block::AIR);
    }

    #[test]
    fn set_block_negative_coordinates() {
        let world = World::new_memory(42);
        world.set_block(-5, 64, -10, block::DIRT);
        assert_eq!(world.get_block(-5, 64, -10), block::DIRT);
    }

    #[test]
    fn persist_chunk_writes_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new(42, dir.path());
        world.set_block(0, 100, 0, block::STONE);
        world.persist_chunk(0, 0);

        // Load from fresh world — should read from disk
        let world2 = World::new(42, dir.path());
        assert_eq!(world2.get_block(0, 100, 0), block::STONE);
    }

    #[test]
    fn set_block_without_persist_is_memory_only() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new(42, dir.path());
        world.set_block(0, 100, 0, block::STONE);
        // No persist_chunk call

        // Fresh world should NOT see the change
        let world2 = World::new(42, dir.path());
        assert_ne!(world2.get_block(0, 100, 0), block::STONE);
    }
}
