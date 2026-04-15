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
pub mod chunk;
pub mod format;
mod generator;
mod noise_gen;
pub mod palette;

pub use chunk::ChunkColumn;
pub use generator::FlatWorldGenerator;
pub use noise_gen::NoiseTerrainGenerator;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use basalt_storage::RegionStorage;
use dashmap::DashMap;

/// Default maximum number of cached chunks when not configured.
const DEFAULT_MAX_CHUNKS: usize = 4096;

/// How terrain is generated for new chunks.
enum Generator {
    /// Superflat world: bedrock + dirt + grass.
    Flat(FlatWorldGenerator),
    /// Noise-based terrain with hills, water, beaches.
    Noise(Box<NoiseTerrainGenerator>),
}

/// A cached chunk with metadata for LRU eviction and dirty tracking.
struct ChunkEntry {
    /// The chunk column data.
    column: ChunkColumn,
    /// Cached encoded packet. Invalidated (set to `None`) when a block changes.
    cached_packet: Option<basalt_protocol::packets::play::world::ClientboundPlayMapChunk>,
    /// Whether this chunk has been modified since the last persist to disk.
    dirty: bool,
    /// Monotonic access counter for approximate LRU eviction.
    last_accessed: u64,
}

/// A Minecraft world with lazy chunk generation, concurrent access,
/// in-memory LRU caching, and optional disk persistence.
///
/// Load order: memory cache -> disk -> generate.
/// Generated chunks are saved to disk (if storage is configured)
/// and cached in memory. When the cache exceeds `max_chunks`, the
/// least recently accessed chunks are evicted (dirty chunks are
/// persisted first).
pub struct World {
    /// Concurrent chunk storage — each chunk independently lockable.
    chunks: DashMap<(i32, i32), ChunkEntry>,
    /// The terrain generator used for new chunks.
    generator: Generator,
    /// The Y coordinate where players spawn.
    spawn_y: i32,
    /// Optional disk storage for chunk persistence.
    storage: Option<RegionStorage>,
    /// Maximum number of chunks kept in the memory cache.
    max_chunks: usize,
    /// Monotonically increasing counter for LRU access tracking.
    tick: AtomicU64,
}

impl World {
    /// Creates a new world with noise-based terrain and disk persistence.
    ///
    /// Chunks are saved to `save_dir/regions/` as BSR region files.
    pub fn new(seed: u32, save_dir: impl Into<PathBuf>) -> Self {
        Self::new_with_capacity(seed, save_dir, DEFAULT_MAX_CHUNKS)
    }

    /// Creates a new world with noise-based terrain, disk persistence,
    /// and a configurable chunk cache limit.
    pub fn new_with_capacity(seed: u32, save_dir: impl Into<PathBuf>, max_chunks: usize) -> Self {
        let save_path = save_dir.into();
        let storage = RegionStorage::new(save_path.join("regions")).ok();
        Self {
            chunks: DashMap::new(),
            generator: Generator::Noise(Box::new(NoiseTerrainGenerator::new(seed))),
            spawn_y: NoiseTerrainGenerator::SPAWN_Y,
            storage,
            max_chunks,
            tick: AtomicU64::new(1),
        }
    }

    /// Creates a new world with noise-based terrain, no persistence.
    pub fn new_memory(seed: u32) -> Self {
        Self::new_memory_with_capacity(seed, DEFAULT_MAX_CHUNKS)
    }

    /// Creates a new world with noise-based terrain, no persistence,
    /// and a configurable chunk cache limit.
    pub fn new_memory_with_capacity(seed: u32, max_chunks: usize) -> Self {
        Self {
            chunks: DashMap::new(),
            generator: Generator::Noise(Box::new(NoiseTerrainGenerator::new(seed))),
            spawn_y: NoiseTerrainGenerator::SPAWN_Y,
            storage: None,
            max_chunks,
            tick: AtomicU64::new(1),
        }
    }

    /// Creates a new flat world (superflat), no persistence.
    pub fn flat() -> Self {
        Self {
            chunks: DashMap::new(),
            generator: Generator::Flat(FlatWorldGenerator),
            spawn_y: FlatWorldGenerator::SPAWN_Y,
            storage: None,
            max_chunks: DEFAULT_MAX_CHUNKS,
            tick: AtomicU64::new(1),
        }
    }

    /// The Y coordinate where players should spawn.
    pub fn spawn_y(&self) -> f64 {
        self.spawn_y as f64
    }

    /// Returns a protocol packet for the chunk at (cx, cz).
    ///
    /// Load order: packet cache -> encode from chunk -> disk -> generate.
    /// Newly generated chunks are saved to disk, stored in memory,
    /// and their encoded packets are cached.
    pub fn get_chunk_packet(
        &self,
        cx: i32,
        cz: i32,
    ) -> basalt_protocol::packets::play::world::ClientboundPlayMapChunk {
        self.ensure_loaded(cx, cz);

        let mut entry = self.chunks.get_mut(&(cx, cz)).unwrap();
        entry.last_accessed = self.tick.fetch_add(1, Ordering::Relaxed);

        if let Some(ref packet) = entry.cached_packet {
            return packet.clone();
        }

        let packet = entry.column.to_packet();
        entry.cached_packet = Some(packet.clone());
        packet
    }

    /// Sets a block at absolute world coordinates.
    ///
    /// Loads the chunk if it isn't already in memory. Invalidates the
    /// packet cache for the affected chunk so the next `get_chunk_packet`
    /// call re-encodes it. Marks the chunk as dirty for persist-before-evict.
    pub fn set_block(&self, x: i32, y: i32, z: i32, state: u16) {
        let cx = x >> 4;
        let cz = z >> 4;
        let local_x = x.rem_euclid(16) as usize;
        let local_z = z.rem_euclid(16) as usize;

        self.ensure_loaded(cx, cz);

        let mut entry = self.chunks.get_mut(&(cx, cz)).unwrap();
        entry.column.set_block(local_x, y, local_z, state);
        entry.cached_packet = None;
        entry.dirty = true;
        entry.last_accessed = self.tick.fetch_add(1, Ordering::Relaxed);
    }

    /// Gets a block at absolute world coordinates.
    ///
    /// Loads the chunk if it isn't already in memory.
    pub fn get_block(&self, x: i32, y: i32, z: i32) -> u16 {
        let cx = x >> 4;
        let cz = z >> 4;
        let local_x = x.rem_euclid(16) as usize;
        let local_z = z.rem_euclid(16) as usize;

        self.ensure_loaded(cx, cz);

        let mut entry = self.chunks.get_mut(&(cx, cz)).unwrap();
        entry.last_accessed = self.tick.fetch_add(1, Ordering::Relaxed);
        entry.column.get_block(local_x, y, local_z)
    }

    /// Persists a chunk to disk via the storage backend.
    ///
    /// Serializes the chunk at (cx, cz) and writes it to the BSR
    /// region file. Clears the dirty flag. No-op if storage is not
    /// configured or the chunk is not loaded.
    pub fn persist_chunk(&self, cx: i32, cz: i32) {
        if let Some(s) = &self.storage
            && let Some(mut entry) = self.chunks.get_mut(&(cx, cz))
        {
            let data = format::serialize_chunk(&entry.column);
            let _ = s.save_raw(cx, cz, &data);
            entry.dirty = false;
        }
    }

    /// Returns true if the chunk at (cx, cz) is in the memory cache.
    pub fn is_chunk_loaded(&self, cx: i32, cz: i32) -> bool {
        self.chunks.contains_key(&(cx, cz))
    }

    /// Returns the number of chunks currently in the memory cache.
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Ensures a chunk is loaded in the cache, generating or loading
    /// from disk if necessary. Triggers eviction if the cache is full.
    fn ensure_loaded(&self, cx: i32, cz: i32) {
        if self.chunks.contains_key(&(cx, cz)) {
            return;
        }

        // Try loading from disk
        if let Some(s) = &self.storage
            && let Ok(Some(data)) = s.load_raw(cx, cz)
            && let Some(col) = format::deserialize_chunk(&data, cx, cz)
        {
            let tick = self.tick.fetch_add(1, Ordering::Relaxed);
            self.chunks.insert(
                (cx, cz),
                ChunkEntry {
                    column: col,
                    cached_packet: None,
                    dirty: false,
                    last_accessed: tick,
                },
            );
            self.evict_if_needed();
            return;
        }

        // Generate new chunk
        let mut col = ChunkColumn::new(cx, cz);
        match &self.generator {
            Generator::Flat(g) => g.generate(&mut col),
            Generator::Noise(g) => g.generate(&mut col),
        }

        // Save to disk
        if let Some(s) = &self.storage {
            let data = format::serialize_chunk(&col);
            let _ = s.save_raw(cx, cz, &data);
        }

        let tick = self.tick.fetch_add(1, Ordering::Relaxed);
        self.chunks.insert(
            (cx, cz),
            ChunkEntry {
                column: col,
                cached_packet: None,
                dirty: false,
                last_accessed: tick,
            },
        );
        self.evict_if_needed();
    }

    /// Evicts the least recently accessed chunks if the cache exceeds
    /// the maximum size. Dirty chunks are persisted to disk before removal.
    fn evict_if_needed(&self) {
        if self.chunks.len() <= self.max_chunks {
            return;
        }

        // How many to evict — remove 10% of max to avoid evicting on every insert
        let target = self.max_chunks * 9 / 10;
        let to_remove = self.chunks.len().saturating_sub(target);
        if to_remove == 0 {
            return;
        }

        // Collect (key, last_accessed) for all entries
        let mut entries: Vec<((i32, i32), u64)> = self
            .chunks
            .iter()
            .map(|r| (*r.key(), r.value().last_accessed))
            .collect();

        // Sort by last_accessed ascending (oldest first)
        entries.sort_by_key(|&(_, tick)| tick);

        // Evict the oldest entries
        for &(key, _) in entries.iter().take(to_remove) {
            if let Some((_, entry)) = self.chunks.remove(&key)
                && entry.dirty
                && let Some(s) = &self.storage
            {
                let data = format::serialize_chunk(&entry.column);
                let _ = s.save_raw(key.0, key.1, &data);
            }
        }
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

    #[test]
    fn eviction_removes_oldest_chunks() {
        // Small cache: max 5 chunks
        let world = World::new_memory_with_capacity(42, 5);

        // Load 6 chunks — should trigger eviction
        for i in 0..6 {
            world.get_chunk_packet(i, 0);
        }

        // Should have evicted down to ~4-5 (90% of 5 = 4)
        assert!(world.chunk_count() <= 5);
    }

    #[test]
    fn eviction_persists_dirty_chunks() {
        let dir = tempfile::tempdir().unwrap();
        // Max 3 chunks
        let world = World::new_with_capacity(42, dir.path(), 3);

        // Load and modify chunk (0,0)
        world.set_block(0, 100, 0, block::STONE);

        // Load 4 more chunks to trigger eviction of (0,0)
        for i in 1..5 {
            world.get_chunk_packet(i, 0);
        }

        // (0,0) should have been evicted and persisted
        let world2 = World::new(42, dir.path());
        assert_eq!(world2.get_block(0, 100, 0), block::STONE);
    }

    #[test]
    fn recently_accessed_chunks_survive_eviction() {
        let world = World::new_memory_with_capacity(42, 5);

        // Load 5 chunks
        for i in 0..5 {
            world.get_chunk_packet(i, 0);
        }

        // Re-access chunk (0,0) to refresh its timestamp
        world.get_chunk_packet(0, 0);

        // Load 2 more to trigger eviction
        world.get_chunk_packet(5, 0);
        world.get_chunk_packet(6, 0);

        // (0,0) should survive — it was recently accessed
        assert!(world.is_chunk_loaded(0, 0));
    }
}
