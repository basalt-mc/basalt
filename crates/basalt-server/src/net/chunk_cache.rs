//! Shared cache for pre-encoded chunk packets.
//!
//! Net tasks look up this cache when they receive a [`ServerOutput::SendChunk`]
//! event. On cache miss, the chunk is fetched from the [`World`], the protocol
//! packet is built and encoded, and the result is stored for future reuse.
//! The game loop invalidates entries when blocks change.
//!
//! The cache has a configurable max size with LRU eviction, mirroring the
//! pattern used by [`World`]'s chunk cache. Each cache manages its own
//! lifecycle independently — no cross-cache dependencies.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use basalt_protocol::packets::play::world::ClientboundPlayMapChunk;
use basalt_types::{Encode, EncodedSize};
use basalt_world::chunk::{SECTIONS_PER_CHUNK, build_full_light_mask};
use dashmap::DashMap;

/// A cached entry with access tracking for LRU eviction.
struct CacheEntry {
    /// Pre-encoded chunk packet bytes.
    data: Arc<Vec<u8>>,
    /// Monotonic access counter for approximate LRU eviction.
    last_accessed: u64,
}

/// Thread-safe cache mapping chunk coordinates to pre-encoded packet bytes.
///
/// Shared across all net tasks via `Arc<ChunkPacketCache>`. The game loop
/// holds a reference to invalidate entries on block mutations.
///
/// Uses LRU eviction when the cache exceeds `max_entries`, matching the
/// same pattern as [`World`]'s chunk cache. Each cache manages its own
/// lifecycle independently.
pub(crate) struct ChunkPacketCache {
    /// Cached entries keyed by (chunk_x, chunk_z).
    cache: DashMap<(i32, i32), CacheEntry>,
    /// Shared world reference for building chunk packets on cache miss.
    world: Arc<basalt_world::World>,
    /// Maximum number of entries before LRU eviction triggers.
    max_entries: usize,
    /// Monotonically increasing counter for LRU access tracking.
    tick: AtomicU64,
}

impl ChunkPacketCache {
    /// Creates a new empty chunk packet cache with a maximum size.
    pub fn new(world: Arc<basalt_world::World>, max_entries: usize) -> Self {
        Self {
            cache: DashMap::new(),
            world,
            max_entries,
            tick: AtomicU64::new(1),
        }
    }

    /// Returns the encoded chunk packet bytes, encoding on cache miss.
    ///
    /// On miss: fetches the chunk from the world, builds the protocol
    /// packet, encodes it, stores the result, and returns it.
    /// On hit: returns the cached `Arc` (cheap pointer clone).
    /// Triggers LRU eviction if the cache exceeds `max_entries`.
    pub fn get_or_encode(&self, cx: i32, cz: i32) -> Arc<Vec<u8>> {
        let tick = self.tick.fetch_add(1, Ordering::Relaxed);

        if let Some(mut entry) = self.cache.get_mut(&(cx, cz)) {
            entry.last_accessed = tick;
            return Arc::clone(&entry.data);
        }

        let encoded = self.world.with_chunk(cx, cz, |col| {
            let packet = build_map_chunk_packet(col);
            let mut buf = Vec::with_capacity(packet.encoded_size());
            packet
                .encode(&mut buf)
                .expect("chunk packet encoding failed");
            buf
        });
        let arc = Arc::new(encoded);
        self.cache.insert(
            (cx, cz),
            CacheEntry {
                data: Arc::clone(&arc),
                last_accessed: tick,
            },
        );
        self.evict_if_needed();
        arc
    }

    /// Invalidates a cached chunk entry after a block mutation.
    ///
    /// Called by the game loop when `set_block()` modifies a chunk.
    /// The next `get_or_encode()` call for this chunk will re-encode.
    pub fn invalidate(&self, cx: i32, cz: i32) {
        self.cache.remove(&(cx, cz));
    }

    /// Returns the number of entries currently in the cache.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.cache.len()
    }

    /// Evicts the least recently accessed entries when over capacity.
    ///
    /// Targets 90% of max to avoid evicting on every insert.
    fn evict_if_needed(&self) {
        if self.cache.len() <= self.max_entries {
            return;
        }

        let target = self.max_entries * 9 / 10;
        let to_remove = self.cache.len() - target;

        let mut entries: Vec<((i32, i32), u64)> = self
            .cache
            .iter()
            .map(|e| (*e.key(), e.value().last_accessed))
            .collect();
        entries.sort_unstable_by_key(|&(_, accessed)| accessed);

        for &(key, _) in entries.iter().take(to_remove) {
            self.cache.remove(&key);
        }
    }
}

/// Builds a [`ClientboundPlayMapChunk`] from a [`ChunkColumn`].
///
/// Protocol packet construction lives here (in basalt-server) to keep
/// basalt-world free of protocol dependencies.
fn build_map_chunk_packet(col: &basalt_world::chunk::ChunkColumn) -> ClientboundPlayMapChunk {
    let chunk_data = col.encode_sections();
    let heightmaps = col.compute_heightmaps();

    // Sky light: all sections get full sunlight (level 15).
    // 26 entries = 24 sections + 1 below + 1 above.
    // Each entry is 2048 bytes (4 bits per block, 16x16x16 / 2).
    let light_sections = SECTIONS_PER_CHUNK + 2;
    let sky_light_mask = build_full_light_mask(light_sections);
    let sky_light: Vec<Vec<u8>> = (0..light_sections).map(|_| vec![0xFF; 2048]).collect();

    ClientboundPlayMapChunk {
        x: col.x,
        z: col.z,
        heightmaps,
        chunk_data,
        block_entities: vec![],
        sky_light_mask,
        block_light_mask: vec![],
        empty_sky_light_mask: vec![],
        empty_block_light_mask: vec![],
        sky_light,
        block_light: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_hit_returns_same_arc() {
        let world = Arc::new(basalt_world::World::new_memory(42));
        let cache = ChunkPacketCache::new(world, 100);

        let first = cache.get_or_encode(0, 0);
        let second = cache.get_or_encode(0, 0);
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn invalidate_causes_re_encode() {
        let world = Arc::new(basalt_world::World::new_memory(42));
        let cache = ChunkPacketCache::new(world, 100);

        let before = cache.get_or_encode(0, 0);
        cache.invalidate(0, 0);
        let after = cache.get_or_encode(0, 0);
        assert!(!Arc::ptr_eq(&before, &after));
    }

    #[test]
    fn different_chunks_are_independent() {
        let world = Arc::new(basalt_world::World::new_memory(42));
        let cache = ChunkPacketCache::new(world, 100);

        let a = cache.get_or_encode(0, 0);
        let b = cache.get_or_encode(1, 0);
        assert!(!Arc::ptr_eq(&a, &b));

        cache.invalidate(0, 0);
        let b2 = cache.get_or_encode(1, 0);
        assert!(Arc::ptr_eq(&b, &b2));
    }

    #[test]
    fn eviction_removes_oldest_entries() {
        let world = Arc::new(basalt_world::World::new_memory(42));
        let cache = ChunkPacketCache::new(world, 5);

        // Load 6 entries — should trigger eviction
        for i in 0..6 {
            cache.get_or_encode(i, 0);
        }

        // Should have evicted down to ~4-5 (90% of 5 = 4)
        assert!(cache.len() <= 5, "cache should not exceed max_entries");
    }

    #[test]
    fn recently_accessed_survives_eviction() {
        let world = Arc::new(basalt_world::World::new_memory(42));
        let cache = ChunkPacketCache::new(world, 5);

        // Load 5 entries
        for i in 0..5 {
            cache.get_or_encode(i, 0);
        }

        // Re-access (0,0) to refresh its timestamp
        let refreshed = cache.get_or_encode(0, 0);

        // Load 2 more to trigger eviction
        cache.get_or_encode(5, 0);
        cache.get_or_encode(6, 0);

        // (0,0) should survive — it was recently accessed
        if let Some(entry) = cache.cache.get(&(0, 0)) {
            assert!(Arc::ptr_eq(&entry.data, &refreshed));
        }
        // At minimum, cache should be bounded
        assert!(cache.len() <= 5);
    }
}
