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
use basalt_api::world::block_entity::BlockEntity;

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
        if section.single_value() != Some(basalt_api::world::block::AIR) {
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

/// Appends block entity data to a BSR buffer.
///
/// Format: `count: u32`, then for each entry:
/// `local_x: u8, y: i16, local_z: u8, type: u8, data...`
///
/// Chest data: 27 × Slot (item_id: i32, count: i32 — simplified).
pub fn serialize_block_entities(
    buf: &mut Vec<u8>,
    block_entities: &[(i32, i32, i32, BlockEntity)],
    chunk_x: i32,
    chunk_z: i32,
) {
    // Filter to entities in this chunk
    let in_chunk: Vec<_> = block_entities
        .iter()
        .filter(|(x, _, z, _)| (x >> 4) == chunk_x && (z >> 4) == chunk_z)
        .collect();

    buf.extend_from_slice(&(in_chunk.len() as u32).to_le_bytes());

    for (x, y, z, be) in in_chunk {
        let local_x = x.rem_euclid(16) as u8;
        let local_z = z.rem_euclid(16) as u8;
        buf.push(local_x);
        buf.extend_from_slice(&(*y as i16).to_le_bytes());
        buf.push(local_z);

        match be {
            BlockEntity::Chest { slots } => {
                buf.push(0); // type 0 = chest
                for slot in slots.iter() {
                    let item_id = slot.item_id.unwrap_or(-1);
                    buf.extend_from_slice(&item_id.to_le_bytes());
                    buf.extend_from_slice(&slot.item_count.to_le_bytes());
                }
            }
        }
    }
}

/// Deserializes block entities from BSR format appended after chunk data.
///
/// Returns `(local_x, y, local_z, BlockEntity)` tuples.
pub fn deserialize_block_entities(
    data: &[u8],
    cursor: &mut usize,
) -> Vec<(u8, i16, u8, BlockEntity)> {
    let mut result = Vec::new();

    if *cursor + 4 > data.len() {
        return result;
    }

    let count = u32::from_le_bytes(data[*cursor..*cursor + 4].try_into().unwrap_or_default());
    *cursor += 4;

    for _ in 0..count {
        if *cursor + 4 > data.len() {
            break;
        }
        let local_x = data[*cursor];
        *cursor += 1;
        let y = i16::from_le_bytes(data[*cursor..*cursor + 2].try_into().unwrap_or_default());
        *cursor += 2;
        let local_z = data[*cursor];
        *cursor += 1;
        let be_type = data[*cursor];
        *cursor += 1;

        match be_type {
            0 => {
                // Chest: 27 slots × (item_id: i32 + count: i32) = 216 bytes
                let mut slots: Box<[basalt_types::Slot; 27]> =
                    Box::new(std::array::from_fn(|_| basalt_types::Slot::empty()));
                for slot in slots.iter_mut() {
                    if *cursor + 8 > data.len() {
                        break;
                    }
                    let item_id = i32::from_le_bytes(
                        data[*cursor..*cursor + 4].try_into().unwrap_or_default(),
                    );
                    *cursor += 4;
                    let item_count = i32::from_le_bytes(
                        data[*cursor..*cursor + 4].try_into().unwrap_or_default(),
                    );
                    *cursor += 4;
                    if item_id >= 0 {
                        *slot = basalt_types::Slot::new(item_id, item_count);
                    }
                }
                result.push((local_x, y, local_z, BlockEntity::Chest { slots }));
            }
            _ => break, // Unknown type, stop
        }
    }

    result
}

/// Deserializes a `ChunkColumn` from BSR binary format.
///
/// The input should be the uncompressed bytes (after LZ4 decompression).
/// Deserializes a `ChunkColumn` from BSR binary format.
///
/// The input should be the uncompressed bytes (after LZ4 decompression).
pub fn deserialize_chunk(data: &[u8], chunk_x: i32, chunk_z: i32) -> Option<ChunkColumn> {
    deserialize_chunk_with_cursor(data, chunk_x, chunk_z).map(|(col, _)| col)
}

/// Deserializes a `ChunkColumn` and returns the cursor position after
/// chunk data, so the caller can continue reading block entities.
pub fn deserialize_chunk_with_cursor(
    data: &[u8],
    chunk_x: i32,
    chunk_z: i32,
) -> Option<(ChunkColumn, usize)> {
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

    Some((chunk, cursor))
}

#[cfg(test)]
mod tests {
    use super::*;
    use basalt_api::world::block;

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

    #[test]
    fn block_entity_roundtrip() {
        let mut slots: Box<[basalt_types::Slot; 27]> =
            Box::new(std::array::from_fn(|_| basalt_types::Slot::empty()));
        slots[0] = basalt_types::Slot::new(1, 10);
        slots[5] = basalt_types::Slot::new(42, 64);

        let bes = vec![(3, 64, 7, BlockEntity::Chest { slots })];

        let mut buf = Vec::new();
        serialize_block_entities(&mut buf, &bes, 0, 0);

        let mut cursor = 0;
        let restored = deserialize_block_entities(&buf, &mut cursor);

        assert_eq!(restored.len(), 1);
        let (lx, y, lz, be) = &restored[0];
        assert_eq!(*lx, 3);
        assert_eq!(*y, 64);
        assert_eq!(*lz, 7);
        match be {
            BlockEntity::Chest { slots } => {
                assert_eq!(slots[0].item_id, Some(1));
                assert_eq!(slots[0].item_count, 10);
                assert_eq!(slots[5].item_id, Some(42));
                assert_eq!(slots[5].item_count, 64);
                assert!(slots[1].is_empty());
            }
        }
    }

    #[test]
    fn empty_block_entities_roundtrip() {
        let bes: Vec<(i32, i32, i32, BlockEntity)> = vec![];
        let mut buf = Vec::new();
        serialize_block_entities(&mut buf, &bes, 0, 0);

        let mut cursor = 0;
        let restored = deserialize_block_entities(&buf, &mut cursor);
        assert!(restored.is_empty());
    }

    #[test]
    fn chunk_with_cursor_returns_correct_position() {
        let chunk = ChunkColumn::new(0, 0);
        let data = serialize_chunk(&chunk);
        let (_, cursor) = deserialize_chunk_with_cursor(&data, 0, 0).unwrap();
        assert_eq!(cursor, 4); // just the bitmap
    }
}
