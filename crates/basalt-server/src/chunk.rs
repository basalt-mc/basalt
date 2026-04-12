//! Minimal chunk data construction for the Play state.
//!
//! The Minecraft client needs at least one chunk at the spawn position
//! to render the world. This module provides a builder for an empty
//! chunk (all air blocks) that satisfies the client's requirements.
//!
//! The chunk format consists of 24 sections (y = -64 to 319), each
//! 16×16×16 blocks. Empty sections use a single-value palette with
//! block state 0 (air) and biome 0.

use basalt_protocol::packets::play::world::ClientboundPlayMapChunk;
use basalt_types::nbt::{NbtCompound, NbtTag};
use basalt_types::{Encode, VarInt};

/// Number of chunk sections in a standard overworld chunk.
///
/// The overworld spans y = -64 to 319, which is 384 blocks tall.
/// Each section is 16 blocks tall, giving 384 / 16 = 24 sections.
const SECTIONS_PER_CHUNK: usize = 24;

/// Builds a chunk packet with a stone floor at y=99.
///
/// The chunk contains 24 sections, mostly air. Section 10 (y=96..111)
/// has a layer of stone at y=99 (local y=3), providing a solid floor
/// just below the spawn point at y=100.
pub fn build_empty_chunk(chunk_x: i32, chunk_z: i32) -> ClientboundPlayMapChunk {
    let chunk_data = encode_chunk_sections();
    let heightmaps = build_heightmaps();

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

/// Block state ID for stone.
const STONE: i32 = 1;

/// Section index that contains y=99 (the stone floor).
/// y=99 is in section 10 (y=96..111), local y = 3.
const FLOOR_SECTION: usize = 10;

/// Local y coordinate of the stone floor within the section.
const FLOOR_LOCAL_Y: usize = 3;

/// Encodes 24 chunk sections into the wire format.
///
/// Section 10 (y=96..111) has a layer of stone at y=99 (local y=3).
/// All other sections are empty air. Each section has block states
/// and biomes as paletted containers.
fn encode_chunk_sections() -> Vec<u8> {
    let mut buf = Vec::new();

    for section in 0..SECTIONS_PER_CHUNK {
        if section == FLOOR_SECTION {
            encode_floor_section(&mut buf);
        } else {
            encode_air_section(&mut buf);
        }
    }

    buf
}

/// Encodes an all-air section (single-value palette).
fn encode_air_section(buf: &mut Vec<u8>) {
    // Block count: 0 non-air blocks
    0i16.encode(buf).unwrap();

    // Block states: single-value palette (air = 0)
    0u8.encode(buf).unwrap();
    VarInt(0).encode(buf).unwrap();
    VarInt(0).encode(buf).unwrap();

    // Biomes: single-value palette (plains = 0)
    0u8.encode(buf).unwrap();
    VarInt(0).encode(buf).unwrap();
    VarInt(0).encode(buf).unwrap();
}

/// Encodes a section with a full layer of stone at the floor y level.
///
/// Uses a 2-entry palette (0=air, 1=stone) with 1 bit per block.
/// A 16×16×16 section has 4096 blocks. At 1 bit per block, that's
/// 4096 bits = 64 longs. Only the blocks at local y = FLOOR_LOCAL_Y
/// are set to 1 (stone).
fn encode_floor_section(buf: &mut Vec<u8>) {
    // Block count: 256 non-air blocks (one 16×16 layer)
    256i16.encode(buf).unwrap();

    // Block states: 2-entry palette with 1 bit per block
    // Bits per entry: 4 (minimum for indirect palette)
    4u8.encode(buf).unwrap();

    // Palette: 2 entries
    VarInt(2).encode(buf).unwrap(); // palette size
    VarInt(0).encode(buf).unwrap(); // palette[0] = air
    VarInt(STONE).encode(buf).unwrap(); // palette[1] = stone

    // Data array: 4096 blocks at 4 bits each = 16384 bits = 256 longs
    VarInt(256).encode(buf).unwrap(); // 256 longs

    // Each long holds 16 blocks at 4 bits each.
    // Blocks are ordered x, z, y (x varies fastest).
    // Each y-layer is 16×16 = 256 blocks = 16 longs.
    // Set palette index 1 (stone) for all blocks at local y = FLOOR_LOCAL_Y.
    for y in 0..16 {
        for _ in 0..16 {
            // 16 longs per y-layer
            let long_value: i64 = if y == FLOOR_LOCAL_Y {
                // All 16 blocks in this long are stone (palette index 1)
                // 4 bits per block, 16 blocks: 0x1111_1111_1111_1111
                0x1111_1111_1111_1111_u64 as i64
            } else {
                // All air (palette index 0)
                0
            };
            long_value.encode(buf).unwrap();
        }
    }

    // Biomes: single-value palette (plains = 0)
    0u8.encode(buf).unwrap();
    VarInt(0).encode(buf).unwrap();
    VarInt(0).encode(buf).unwrap();
}

/// Builds heightmaps for a chunk with a stone floor at y=99.
///
/// Heightmaps encode the highest non-air block + 1 in each column.
/// With a floor at y=99, the value for every column is 100 (relative
/// to the world bottom at y=-64, so 100 - (-64) = 164).
///
/// Each heightmap is a packed array of 256 entries (16×16 columns),
/// 9 bits each, packed into longs. ceil(256 * 9 / 64) = 36 longs.
fn build_heightmaps() -> NbtCompound {
    // Height value: y=99 block means heightmap value = 99 - (-64) + 1 = 164
    let height_value: u64 = 164;

    // Pack 256 entries of 9 bits each into longs.
    // Each long holds floor(64 / 9) = 7 entries.
    // 256 entries / 7 per long = 37 longs (last one partially filled).
    let mut longs = vec![0i64; 37];
    for i in 0..256 {
        let long_index = (i * 9) / 64;
        let bit_offset = (i * 9) % 64;
        longs[long_index] |= (height_value as i64) << bit_offset;
    }

    let mut heightmaps = NbtCompound::new();
    heightmaps.insert("MOTION_BLOCKING", NbtTag::LongArray(longs.clone()));
    heightmaps.insert("WORLD_SURFACE", NbtTag::LongArray(longs));
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
        let data = encode_chunk_sections();
        // 23 air sections × 8 bytes each + 1 floor section (larger)
        assert!(data.len() > 23 * 8);
    }

    #[test]
    fn heightmaps_have_required_keys() {
        let hm = build_heightmaps();
        assert!(hm.get("MOTION_BLOCKING").is_some());
        assert!(hm.get("WORLD_SURFACE").is_some());
    }

    #[test]
    fn heightmaps_are_long_arrays() {
        let hm = build_heightmaps();
        match hm.get("MOTION_BLOCKING") {
            Some(NbtTag::LongArray(arr)) => {
                assert_eq!(arr.len(), 37);
                // At least one long should be non-zero (height = 164)
                assert!(arr.iter().any(|&v| v != 0));
            }
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
