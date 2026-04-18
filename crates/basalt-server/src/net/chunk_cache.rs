//! Shared cache for pre-encoded chunk packets.
//!
//! Net tasks look up this cache when they receive a [`ServerOutput::SendChunk`]
//! event. On cache miss, the chunk is fetched from the [`World`], the protocol
//! packet is built and encoded, and the result is stored for future reuse.
//! The game loop invalidates entries when blocks change.

use std::sync::Arc;

use basalt_protocol::packets::play::world::ClientboundPlayMapChunk;
use basalt_types::{Encode, EncodedSize};
use basalt_world::chunk::{SECTIONS_PER_CHUNK, build_full_light_mask};
use dashmap::DashMap;

/// Thread-safe cache mapping chunk coordinates to pre-encoded packet bytes.
///
/// Shared across all net tasks via `Arc<ChunkPacketCache>`. The game loop
/// holds a reference to invalidate entries on block mutations.
pub(crate) struct ChunkPacketCache {
    /// Encoded chunk packet bytes keyed by (chunk_x, chunk_z).
    cache: DashMap<(i32, i32), Arc<Vec<u8>>>,
    /// Shared world reference for building chunk packets on cache miss.
    world: Arc<basalt_world::World>,
}

impl ChunkPacketCache {
    /// Creates a new empty chunk packet cache.
    pub fn new(world: Arc<basalt_world::World>) -> Self {
        Self {
            cache: DashMap::new(),
            world,
        }
    }

    /// Returns the encoded chunk packet bytes, encoding on cache miss.
    ///
    /// On miss: fetches the chunk from the world, builds the protocol
    /// packet, encodes it, stores the result, and returns it.
    /// On hit: returns the cached `Arc` (cheap pointer clone).
    pub fn get_or_encode(&self, cx: i32, cz: i32) -> Arc<Vec<u8>> {
        if let Some(entry) = self.cache.get(&(cx, cz)) {
            return Arc::clone(entry.value());
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
        self.cache.insert((cx, cz), Arc::clone(&arc));
        arc
    }

    /// Invalidates a cached chunk entry after a block mutation.
    ///
    /// Called by the game loop when `set_block()` modifies a chunk.
    /// The next `get_or_encode()` call for this chunk will re-encode.
    pub fn invalidate(&self, cx: i32, cz: i32) {
        self.cache.remove(&(cx, cz));
    }
}

/// Builds a [`ClientboundPlayMapChunk`] from a [`ChunkColumn`].
///
/// This is the protocol packet construction that was previously in
/// `ChunkColumn::to_packet()`. It lives in basalt-server to keep
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
        let cache = ChunkPacketCache::new(world);

        let first = cache.get_or_encode(0, 0);
        let second = cache.get_or_encode(0, 0);
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn invalidate_causes_re_encode() {
        let world = Arc::new(basalt_world::World::new_memory(42));
        let cache = ChunkPacketCache::new(world);

        let before = cache.get_or_encode(0, 0);
        cache.invalidate(0, 0);
        let after = cache.get_or_encode(0, 0);
        assert!(!Arc::ptr_eq(&before, &after));
    }

    #[test]
    fn different_chunks_are_independent() {
        let world = Arc::new(basalt_world::World::new_memory(42));
        let cache = ChunkPacketCache::new(world);

        let a = cache.get_or_encode(0, 0);
        let b = cache.get_or_encode(1, 0);
        assert!(!Arc::ptr_eq(&a, &b));

        cache.invalidate(0, 0);
        // (1,0) should still be cached
        let b2 = cache.get_or_encode(1, 0);
        assert!(Arc::ptr_eq(&b, &b2));
    }
}
