//! BSR (Basalt Region) chunk serialization format.
//!
//! Serializes `ChunkColumn` into a compact binary format optimized
//! for LZ4 compression. Single-value sections (all air, all stone)
//! are stored as 3 bytes instead of 8KB.
//!
//! # Wire format (before compression)
//!
//! ```text
//! section_bitmap: u32          — which sections have data (bit per section)
//! for each set bit:
//!   section_type: u8           — 0 = single-value, 1 = paletted
//!   if single-value:
//!     block_id: u16            — the single block state (3 bytes total!)
//!   if paletted:
//!     block_data: [u16; 4096]  — raw block state IDs (8KB)
//! ```

use crate::chunk::{ChunkColumn, SECTIONS_PER_CHUNK};
use crate::palette::PalettedContainer;

/// Section type: all blocks are the same state.
const SECTION_SINGLE_VALUE: u8 = 0;
/// Section type: mixed blocks, stored as raw u16 array.
const SECTION_RAW: u8 = 1;

/// Serializes a `ChunkColumn` into the BSR binary format.
///
/// Returns the uncompressed bytes. The caller is responsible for
/// LZ4 compression before writing to disk.
pub fn serialize_chunk(chunk: &ChunkColumn) -> Vec<u8> {
    let mut buf = Vec::new();

    // Section bitmap — which sections have non-air data
    let mut bitmap: u32 = 0;
    for (i, section) in chunk.sections.iter().enumerate() {
        if section.single_value() != Some(crate::block::AIR) {
            bitmap |= 1 << i;
        }
    }
    buf.extend_from_slice(&bitmap.to_le_bytes());

    // Serialize each present section
    for (i, section) in chunk.sections.iter().enumerate() {
        if bitmap & (1 << i) == 0 {
            continue; // Skip all-air sections
        }

        if let Some(state) = section.single_value() {
            // Single-value section: 3 bytes
            buf.push(SECTION_SINGLE_VALUE);
            buf.extend_from_slice(&state.to_le_bytes());
        } else {
            // Raw section: 1 + 8192 bytes
            buf.push(SECTION_RAW);
            for &block in section.blocks() {
                buf.extend_from_slice(&block.to_le_bytes());
            }
        }
    }

    buf
}

/// Deserializes a `ChunkColumn` from BSR binary format.
///
/// The input should be the uncompressed bytes (after LZ4 decompression).
pub fn deserialize_chunk(data: &[u8], chunk_x: i32, chunk_z: i32) -> Option<ChunkColumn> {
    let mut cursor = 0;

    if data.len() < 4 {
        return None;
    }

    // Read section bitmap
    let bitmap = u32::from_le_bytes(data[cursor..cursor + 4].try_into().ok()?);
    cursor += 4;

    let mut chunk = ChunkColumn::new(chunk_x, chunk_z);

    for i in 0..SECTIONS_PER_CHUNK {
        if bitmap & (1 << i) == 0 {
            continue; // Section is all air (already default)
        }

        if cursor >= data.len() {
            return None;
        }

        let section_type = data[cursor];
        cursor += 1;

        match section_type {
            SECTION_SINGLE_VALUE => {
                if cursor + 2 > data.len() {
                    return None;
                }
                let state = u16::from_le_bytes(data[cursor..cursor + 2].try_into().ok()?);
                cursor += 2;
                chunk.sections[i] = PalettedContainer::filled(state);
            }
            SECTION_RAW => {
                if cursor + 8192 > data.len() {
                    return None;
                }
                let mut blocks = [0u16; 4096];
                for (j, block) in blocks.iter_mut().enumerate() {
                    let offset = cursor + j * 2;
                    *block = u16::from_le_bytes(data[offset..offset + 2].try_into().ok()?);
                }
                cursor += 8192;
                chunk.sections[i] = PalettedContainer::from_blocks(blocks);
            }
            _ => return None, // Unknown section type
        }
    }

    Some(chunk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block;

    #[test]
    fn empty_chunk_roundtrip() {
        let chunk = ChunkColumn::new(0, 0);
        let data = serialize_chunk(&chunk);
        // All-air chunk: just the bitmap (4 bytes, all zeros)
        assert_eq!(data.len(), 4);
        assert_eq!(data, [0, 0, 0, 0]);

        let restored = deserialize_chunk(&data, 0, 0).unwrap();
        assert_eq!(restored.get_block(0, 0, 0), block::AIR);
    }

    #[test]
    fn single_value_section_is_compact() {
        let mut chunk = ChunkColumn::new(0, 0);
        // Fill section 0 (y=-64..-48) entirely with stone
        for x in 0..16 {
            for z in 0..16 {
                for y in -64..-48 {
                    chunk.set_block(x, y, z, block::STONE);
                }
            }
        }

        let data = serialize_chunk(&chunk);
        // Bitmap (4) + single-value section (1 type + 2 block_id) = 7 bytes
        assert_eq!(data.len(), 7);
    }

    #[test]
    fn mixed_section_roundtrip() {
        let mut chunk = ChunkColumn::new(3, -5);
        chunk.set_block(0, -64, 0, block::BEDROCK);
        chunk.set_block(1, -64, 0, block::STONE);
        chunk.set_block(8, 64, 8, block::GRASS_BLOCK);

        let data = serialize_chunk(&chunk);
        let restored = deserialize_chunk(&data, 3, -5).unwrap();

        assert_eq!(restored.get_block(0, -64, 0), block::BEDROCK);
        assert_eq!(restored.get_block(1, -64, 0), block::STONE);
        assert_eq!(restored.get_block(8, 64, 8), block::GRASS_BLOCK);
        assert_eq!(restored.get_block(0, 0, 0), block::AIR);
    }

    #[test]
    fn generated_chunk_roundtrip() {
        let generator = crate::FlatWorldGenerator;
        let mut chunk = ChunkColumn::new(0, 0);
        generator.generate(&mut chunk);

        let data = serialize_chunk(&chunk);
        let restored = deserialize_chunk(&data, 0, 0).unwrap();

        // Verify terrain is identical
        assert_eq!(restored.get_block(0, -64, 0), block::BEDROCK);
        assert_eq!(restored.get_block(0, -63, 0), block::DIRT);
        assert_eq!(restored.get_block(0, -61, 0), block::GRASS_BLOCK);
        assert_eq!(restored.get_block(0, -60, 0), block::AIR);
    }
}
