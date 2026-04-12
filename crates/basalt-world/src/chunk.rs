//! Chunk column representation and wire encoding.
//!
//! A `ChunkColumn` holds 24 sections (y = -64 to 319) and provides
//! methods to set blocks and encode the column into a protocol packet.

use basalt_protocol::packets::play::world::ClientboundPlayMapChunk;
use basalt_types::nbt::{NbtCompound, NbtTag};
use basalt_types::{Encode, VarInt};

use crate::block;

/// Builds a BitSet where the first `count` bits are set.
///
/// The Minecraft sky light mask is a BitSet encoded as a VarInt length
/// followed by an array of i64 values. Each bit indicates whether the
/// corresponding section has light data.
fn build_full_light_mask(count: usize) -> Vec<i64> {
    let num_longs = count.div_ceil(64);
    let mut longs = vec![0i64; num_longs];
    for i in 0..count {
        longs[i / 64] |= 1i64 << (i % 64);
    }
    longs
}
use crate::palette::PalettedContainer;

/// Number of chunk sections in a standard overworld chunk.
pub const SECTIONS_PER_CHUNK: usize = 24;

/// The Y coordinate of the bottom of the world.
const WORLD_BOTTOM_Y: i32 = -64;

/// A vertical column of 24 chunk sections at a given (x, z) position.
pub struct ChunkColumn {
    /// Chunk X coordinate (in chunk units, not blocks).
    pub x: i32,
    /// Chunk Z coordinate.
    pub z: i32,
    /// The 24 sections from bottom (y=-64) to top (y=319).
    /// Boxed to avoid 192KB stack allocation (24 × 8KB per section).
    pub sections: Box<[PalettedContainer; SECTIONS_PER_CHUNK]>,
}

impl ChunkColumn {
    /// Creates a new chunk column filled entirely with air.
    pub fn new(x: i32, z: i32) -> Self {
        Self {
            x,
            z,
            sections: Box::new(std::array::from_fn(|_| {
                PalettedContainer::filled(block::AIR)
            })),
        }
    }

    /// Sets a block at absolute world coordinates within this chunk.
    ///
    /// The x and z must be within 0..16 (local to the chunk).
    /// The y is in absolute world coordinates (-64 to 319).
    pub fn set_block(&mut self, x: usize, y: i32, z: usize, state: u16) {
        let section_index = ((y - WORLD_BOTTOM_Y) / 16) as usize;
        let local_y = ((y - WORLD_BOTTOM_Y) % 16) as usize;
        if section_index < SECTIONS_PER_CHUNK {
            self.sections[section_index].set(x, local_y, z, state);
        }
    }

    /// Gets a block at absolute world coordinates within this chunk.
    pub fn get_block(&self, x: usize, y: i32, z: usize) -> u16 {
        let section_index = ((y - WORLD_BOTTOM_Y) / 16) as usize;
        let local_y = ((y - WORLD_BOTTOM_Y) % 16) as usize;
        if section_index < SECTIONS_PER_CHUNK {
            self.sections[section_index].get(x, local_y, z)
        } else {
            block::AIR
        }
    }

    /// Converts this chunk column into a protocol packet ready to send.
    pub fn to_packet(&self) -> ClientboundPlayMapChunk {
        let chunk_data = self.encode_sections();
        let heightmaps = self.compute_heightmaps();

        // Sky light: all sections get full sunlight (level 15).
        // The light system has 26 entries (24 sections + 1 below + 1 above).
        // Each entry is a 2048-byte array (4 bits per block, 16×16×16 / 2).
        let light_sections = SECTIONS_PER_CHUNK + 2;
        // BitSet marking which sections have sky light data: all of them
        let sky_light_mask = build_full_light_mask(light_sections);
        // Full sunlight = all nibbles set to 15 (0xFF bytes)
        let sky_light: Vec<Vec<u8>> = (0..light_sections).map(|_| vec![0xFF; 2048]).collect();

        ClientboundPlayMapChunk {
            x: self.x,
            z: self.z,
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

    /// Encodes all 24 sections into the wire format.
    fn encode_sections(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        for section in self.sections.iter() {
            // Block count
            section.non_air_count().encode(&mut buf).unwrap();
            // Block states paletted container
            section.encode_to(&mut buf);
            // Biomes: single-value palette (plains = 0)
            0u8.encode(&mut buf).unwrap();
            VarInt(0).encode(&mut buf).unwrap();
            VarInt(0).encode(&mut buf).unwrap();
        }
        buf
    }

    /// Computes MOTION_BLOCKING and WORLD_SURFACE heightmaps.
    ///
    /// For each column (x, z), finds the highest non-air block and
    /// stores height + 1 (relative to world bottom) as a 9-bit value
    /// packed into longs.
    fn compute_heightmaps(&self) -> NbtCompound {
        let mut heights = [0u64; 256];

        for x in 0..16 {
            for z in 0..16 {
                // Scan from top to bottom to find highest non-air
                for y in (WORLD_BOTTOM_Y..(WORLD_BOTTOM_Y + 384)).rev() {
                    if self.get_block(x, y, z) != block::AIR {
                        // Height value = y - WORLD_BOTTOM_Y + 1
                        heights[z * 16 + x] = (y - WORLD_BOTTOM_Y + 1) as u64;
                        break;
                    }
                }
            }
        }

        // Pack 256 entries of 9 bits each into longs
        let mut longs = vec![0i64; 37];
        for (i, &height) in heights.iter().enumerate() {
            let long_index = (i * 9) / 64;
            let bit_offset = (i * 9) % 64;
            longs[long_index] |= (height as i64) << bit_offset;
        }

        let mut heightmaps = NbtCompound::new();
        heightmaps.insert("MOTION_BLOCKING", NbtTag::LongArray(longs.clone()));
        heightmaps.insert("WORLD_SURFACE", NbtTag::LongArray(longs));
        heightmaps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_chunk_is_all_air() {
        let chunk = ChunkColumn::new(0, 0);
        assert_eq!(chunk.get_block(0, 0, 0), block::AIR);
        assert_eq!(chunk.get_block(8, 64, 8), block::AIR);
    }

    #[test]
    fn set_and_get_block() {
        let mut chunk = ChunkColumn::new(0, 0);
        chunk.set_block(5, 10, 3, block::STONE);
        assert_eq!(chunk.get_block(5, 10, 3), block::STONE);
        assert_eq!(chunk.get_block(0, 10, 0), block::AIR);
    }

    #[test]
    fn set_block_negative_y() {
        let mut chunk = ChunkColumn::new(0, 0);
        chunk.set_block(0, -64, 0, block::BEDROCK);
        assert_eq!(chunk.get_block(0, -64, 0), block::BEDROCK);
    }

    #[test]
    fn to_packet_produces_valid_data() {
        let mut chunk = ChunkColumn::new(3, -5);
        chunk.set_block(0, 0, 0, block::BEDROCK);
        let packet = chunk.to_packet();
        assert_eq!(packet.x, 3);
        assert_eq!(packet.z, -5);
        assert!(!packet.chunk_data.is_empty());
    }

    #[test]
    fn heightmap_reflects_blocks() {
        let mut chunk = ChunkColumn::new(0, 0);
        chunk.set_block(8, 64, 8, block::STONE);
        let hm = chunk.compute_heightmaps();
        match hm.get("MOTION_BLOCKING") {
            Some(NbtTag::LongArray(arr)) => {
                assert!(arr.iter().any(|&v| v != 0));
            }
            _ => panic!("expected LongArray"),
        }
    }
}
