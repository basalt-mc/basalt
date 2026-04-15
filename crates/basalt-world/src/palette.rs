//! Paletted container for chunk section block storage.
//!
//! Minecraft uses paletted containers to compress block storage.
//! Instead of storing a full block state ID (16 bits) per block,
//! sections maintain a palette of unique block states and store
//! palette indices using the minimum number of bits.
//!
//! This module handles encoding the paletted container into the
//! wire format expected by the Minecraft client.

use std::collections::HashMap;

use basalt_types::{Encode, VarInt};

/// Maximum bits per entry for indirect palette. Above this, use direct mode.
const MAX_INDIRECT_BITS: u8 = 8;

/// A 16×16×16 section of blocks stored as a paletted container.
///
/// Blocks are stored as raw state IDs. The palette is computed
/// during encoding to determine the optimal bits-per-entry.
pub struct PalettedContainer {
    /// Block state IDs in XZY order (x varies fastest, then z, then y).
    /// Length must be 4096 (16×16×16).
    blocks: [u16; 4096],
}

impl PalettedContainer {
    /// Creates a container filled with a single block state.
    pub fn filled(state: u16) -> Self {
        Self {
            blocks: [state; 4096],
        }
    }

    /// Sets the block at the given local coordinates.
    ///
    /// Coordinates must be in 0..16. The storage order is XZY:
    /// index = (y * 16 + z) * 16 + x.
    pub fn set(&mut self, x: usize, y: usize, z: usize, state: u16) {
        self.blocks[(y * 16 + z) * 16 + x] = state;
    }

    /// Gets the block at the given local coordinates.
    pub fn get(&self, x: usize, y: usize, z: usize) -> u16 {
        self.blocks[(y * 16 + z) * 16 + x]
    }

    /// Counts the number of non-air blocks in this container.
    pub fn non_air_count(&self) -> i16 {
        self.blocks
            .iter()
            .filter(|&&state| state != crate::block::AIR)
            .count() as i16
    }

    /// Returns true if all blocks are the same state (single-value).
    pub fn is_single_value(&self) -> bool {
        let first = self.blocks[0];
        self.blocks.iter().all(|&b| b == first)
    }

    /// Returns the single block state if this container is homogeneous.
    pub fn single_value(&self) -> Option<u16> {
        if self.is_single_value() {
            Some(self.blocks[0])
        } else {
            None
        }
    }

    /// Returns a reference to the raw block state array.
    pub fn blocks(&self) -> &[u16; 4096] {
        &self.blocks
    }

    /// Creates a container from a raw block state array.
    pub fn from_blocks(blocks: [u16; 4096]) -> Self {
        Self { blocks }
    }

    /// Encodes this container into the Minecraft wire format.
    ///
    /// The format depends on the number of unique block states:
    /// - **Single-value** (1 state): bits=0, one VarInt palette entry, no data
    /// - **Indirect** (2-256 states, bits 4-8): palette + packed indices
    /// - **Direct** (>256 states, bits=15): no palette, raw global state IDs
    ///
    /// Uses a HashMap for O(1) palette index lookups instead of O(n) linear scan.
    pub fn encode_to(&self, buf: &mut Vec<u8>) {
        // Build palette with O(1) lookup
        let mut palette: Vec<u16> = Vec::new();
        let mut palette_map: HashMap<u16, usize> = HashMap::new();
        for &state in &self.blocks {
            if let std::collections::hash_map::Entry::Vacant(e) = palette_map.entry(state) {
                e.insert(palette.len());
                palette.push(state);
            }
        }

        if palette.len() == 1 {
            // Single-value palette — no data array needed
            0u8.encode(buf).unwrap();
            VarInt(palette[0] as i32).encode(buf).unwrap();
            VarInt(0).encode(buf).unwrap(); // data array length = 0
            return;
        }

        let raw_bits = bits_needed(palette.len() as u32);

        if raw_bits <= MAX_INDIRECT_BITS {
            // Indirect palette — bits 4-8
            let bits_per_entry = std::cmp::max(4, raw_bits);
            bits_per_entry.encode(buf).unwrap();

            // Palette entries
            VarInt(palette.len() as i32).encode(buf).unwrap();
            for &state in &palette {
                VarInt(state as i32).encode(buf).unwrap();
            }

            // Pack palette indices into longs
            self.encode_packed_longs(buf, bits_per_entry, |state| palette_map[&state] as u64);
        } else {
            // Direct mode — 15 bits per entry, no palette
            let bits_per_entry: u8 = 15;
            bits_per_entry.encode(buf).unwrap();
            self.encode_packed_longs(buf, bits_per_entry, |state| state as u64);
        }
    }

    /// Packs block values into longs at the given bits-per-entry.
    fn encode_packed_longs(
        &self,
        buf: &mut Vec<u8>,
        bits_per_entry: u8,
        value_fn: impl Fn(u16) -> u64,
    ) {
        let entries_per_long = 64 / bits_per_entry as usize;
        let num_longs = 4096_usize.div_ceil(entries_per_long);
        let mask = (1u64 << bits_per_entry) - 1;

        VarInt(num_longs as i32).encode(buf).unwrap();

        let mut long_value: u64 = 0;
        let mut bit_offset = 0;
        let mut longs_written = 0;

        for &state in &self.blocks {
            long_value |= (value_fn(state) & mask) << bit_offset;
            bit_offset += bits_per_entry as usize;

            if bit_offset >= 64 {
                (long_value as i64).encode(buf).unwrap();
                longs_written += 1;
                long_value = 0;
                bit_offset = 0;
            }
        }

        if bit_offset > 0 {
            (long_value as i64).encode(buf).unwrap();
            longs_written += 1;
        }

        for _ in longs_written..num_longs {
            0i64.encode(buf).unwrap();
        }
    }
}

/// Returns the number of bits needed to represent `n` values.
fn bits_needed(n: u32) -> u8 {
    if n <= 1 {
        return 0;
    }
    (32 - (n - 1).leading_zeros()) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block;

    #[test]
    fn single_value_palette() {
        let container = PalettedContainer::filled(block::AIR);
        let mut buf = Vec::new();
        container.encode_to(&mut buf);
        // Single-value: bits_per_entry(0) + palette(VarInt 0) + data_len(VarInt 0)
        assert_eq!(buf.len(), 3);
    }

    #[test]
    fn two_block_palette() {
        let mut container = PalettedContainer::filled(block::AIR);
        container.set(0, 0, 0, block::STONE);
        let mut buf = Vec::new();
        container.encode_to(&mut buf);
        // Should have indirect palette with 2 entries
        assert!(buf.len() > 3);
        assert_eq!(container.non_air_count(), 1);
    }

    #[test]
    fn full_layer_count() {
        let mut container = PalettedContainer::filled(block::AIR);
        for x in 0..16 {
            for z in 0..16 {
                container.set(x, 3, z, block::STONE);
            }
        }
        assert_eq!(container.non_air_count(), 256);
    }

    #[test]
    fn get_set_roundtrip() {
        let mut container = PalettedContainer::filled(block::AIR);
        container.set(5, 10, 3, block::BEDROCK);
        assert_eq!(container.get(5, 10, 3), block::BEDROCK);
        assert_eq!(container.get(0, 0, 0), block::AIR);
    }

    #[test]
    fn direct_mode_many_unique_states() {
        // >256 unique states forces direct mode (15 bits, no palette)
        let mut container = PalettedContainer::filled(0);
        for i in 0..4096u16 {
            // Use 300 unique states to exceed the 256 indirect limit
            container.set(
                i as usize % 16,
                i as usize / 256,
                (i as usize / 16) % 16,
                i % 300,
            );
        }
        let mut buf = Vec::new();
        container.encode_to(&mut buf);
        // First byte should be 15 (direct mode bits per entry)
        assert_eq!(buf[0], 15);
        // Should not panic and produce valid output
        assert!(buf.len() > 100);
    }

    #[test]
    fn bits_needed_values() {
        assert_eq!(bits_needed(1), 0);
        assert_eq!(bits_needed(2), 1);
        assert_eq!(bits_needed(3), 2);
        assert_eq!(bits_needed(4), 2);
        assert_eq!(bits_needed(5), 3);
        assert_eq!(bits_needed(16), 4);
        assert_eq!(bits_needed(17), 5);
    }
}
