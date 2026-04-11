//! Minimal chunk data construction for the Play state.
//!
//! The Minecraft client needs at least one chunk at the spawn position
//! to render the world. This module provides a builder for an empty
//! chunk (all air blocks) that satisfies the client's requirements.
//!
//! The chunk format consists of 24 sections (y = -64 to 319), each
//! 16×16×16 blocks. Empty sections use a single-value palette with
//! block state 0 (air) and biome 0.

use crate::packets::play::world::ClientboundPlayMapChunk;
use basalt_types::nbt::{NbtCompound, NbtTag};
use basalt_types::{Encode, VarInt};

/// Number of chunk sections in a standard overworld chunk.
///
/// The overworld spans y = -64 to 319, which is 384 blocks tall.
/// Each section is 16 blocks tall, giving 384 / 16 = 24 sections.
const SECTIONS_PER_CHUNK: usize = 24;

/// Builds an empty chunk packet at the given chunk coordinates.
///
/// The chunk contains 24 sections of air with plains biome. Heightmaps
/// are empty (all zeros). No block entities, no light data. This is the
/// minimum the client needs to render a void world.
pub fn build_empty_chunk(chunk_x: i32, chunk_z: i32) -> ClientboundPlayMapChunk {
    let chunk_data = encode_empty_chunk_sections();
    let heightmaps = build_empty_heightmaps();

    ClientboundPlayMapChunk {
        x: chunk_x,
        z: chunk_z,
        heightmaps,
        chunk_data,
        block_entities: vec![],
        sky_light_mask: vec![],
        block_light_mask: vec![],
        empty_sky_light_mask: vec![],
        empty_block_light_mask: vec![],
        sky_light: vec![],
        block_light: vec![],
    }
}

/// Encodes 24 empty chunk sections into the wire format.
///
/// Each section contains:
/// - Block count (i16): 0 — no non-air blocks
/// - Block states paletted container:
///   - Bits per entry (u8): 0 — single-value palette
///   - Palette entry (VarInt): 0 — air block state
///   - Data array length (VarInt): 0 — no data needed
/// - Biomes paletted container:
///   - Bits per entry (u8): 0 — single-value palette
///   - Palette entry (VarInt): 0 — first biome (plains)
///   - Data array length (VarInt): 0 — no data needed
fn encode_empty_chunk_sections() -> Vec<u8> {
    let mut buf = Vec::new();

    for _ in 0..SECTIONS_PER_CHUNK {
        // Block count: 0 non-air blocks
        0i16.encode(&mut buf).unwrap();

        // Block states: single-value palette (air = 0)
        0u8.encode(&mut buf).unwrap(); // bits per entry
        VarInt(0).encode(&mut buf).unwrap(); // palette: air
        VarInt(0).encode(&mut buf).unwrap(); // data array length: 0

        // Biomes: single-value palette (first biome = 0)
        0u8.encode(&mut buf).unwrap(); // bits per entry
        VarInt(0).encode(&mut buf).unwrap(); // palette: plains
        VarInt(0).encode(&mut buf).unwrap(); // data array length: 0
    }

    buf
}

/// Builds empty heightmaps as an NBT compound.
///
/// Heightmaps are LONG_ARRAY tags encoding the highest non-air block
/// in each column. For an empty chunk, all values are 0. The client
/// expects at least MOTION_BLOCKING and WORLD_SURFACE heightmaps.
///
/// Each heightmap is a packed array of 256 entries (16×16 columns),
/// 9 bits each, packed into longs. For all-zero values, we need
/// ceil(256 * 9 / 64) = 36 longs, all set to 0.
fn build_empty_heightmaps() -> NbtCompound {
    let empty_heightmap = vec![0i64; 37];

    let mut heightmaps = NbtCompound::new();
    heightmaps.insert(
        "MOTION_BLOCKING",
        NbtTag::LongArray(empty_heightmap.clone()),
    );
    heightmaps.insert("WORLD_SURFACE", NbtTag::LongArray(empty_heightmap));
    heightmaps
}

#[cfg(test)]
mod tests {
    use super::*;
    use basalt_types::EncodedSize;

    #[test]
    fn empty_chunk_at_origin() {
        let chunk = build_empty_chunk(0, 0);
        assert_eq!(chunk.x, 0);
        assert_eq!(chunk.z, 0);
        assert!(!chunk.chunk_data.is_empty());
        assert!(chunk.block_entities.is_empty());
    }

    #[test]
    fn chunk_data_has_24_sections() {
        let data = encode_empty_chunk_sections();
        // Each section: i16(2) + u8(1) + VarInt(1) + VarInt(1) + u8(1) + VarInt(1) + VarInt(1) = 8 bytes
        assert_eq!(data.len(), 24 * 8);
    }

    #[test]
    fn heightmaps_have_required_keys() {
        let hm = build_empty_heightmaps();
        assert!(hm.get("MOTION_BLOCKING").is_some());
        assert!(hm.get("WORLD_SURFACE").is_some());
    }

    #[test]
    fn heightmaps_are_long_arrays() {
        let hm = build_empty_heightmaps();
        match hm.get("MOTION_BLOCKING") {
            Some(NbtTag::LongArray(arr)) => assert_eq!(arr.len(), 37),
            other => panic!("expected LongArray, got {:?}", other),
        }
    }

    #[test]
    fn chunk_encodes() {
        let chunk = build_empty_chunk(0, 0);
        let mut buf = Vec::with_capacity(chunk.encoded_size());
        chunk.encode(&mut buf).unwrap();
        assert!(!buf.is_empty());
    }

    #[test]
    fn chunk_at_different_coords() {
        let chunk = build_empty_chunk(3, -5);
        assert_eq!(chunk.x, 3);
        assert_eq!(chunk.z, -5);
    }
}
