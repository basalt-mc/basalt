//! Paletted container for chunk section block storage.
//!
//! Minecraft uses paletted containers to compress block storage.
//! Instead of storing a full block state ID (16 bits) per block,
//! sections maintain a palette of unique block states and store
//! palette indices using the minimum number of bits.
//!
//! This module handles encoding the paletted container into the
//! wire format expected by the Minecraft client.

use basalt_types::{Encode, VarInt};

/// A 16×16×16 section of blocks stored as a paletted container.
///
/// Blocks are stored as raw state IDs. The palette is computed
/// during encoding to determine the optimal bits-per-entry.
pub(crate) struct PalettedContainer {
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

    /// Encodes this container into the Minecraft wire format.
    ///
    /// The format is:
    /// - Bits per entry (u8): 0 for single-value, 4-8 for indirect
    /// - Palette: VarInt count + VarInt entries (for indirect)
    ///   or single VarInt entry (for single-value)
    /// - Data array: VarInt length + packed longs
    pub fn encode_to(&self, buf: &mut Vec<u8>) {
        // Build palette — collect unique block states
        let mut palette: Vec<u16> = Vec::new();
        for &state in &self.blocks {
            if !palette.contains(&state) {
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

        // Indirect palette — compute bits per entry
        // Minimum is 4 for block states
        let bits_per_entry = std::cmp::max(4, bits_needed(palette.len() as u32));

        bits_per_entry.encode(buf).unwrap();

        // Palette entries
        VarInt(palette.len() as i32).encode(buf).unwrap();
        for &state in &palette {
            VarInt(state as i32).encode(buf).unwrap();
        }

        // Pack blocks into longs
        let entries_per_long = 64 / bits_per_entry as usize;
        let num_longs = 4096_usize.div_ceil(entries_per_long);
        let mask = (1u64 << bits_per_entry) - 1;

        VarInt(num_longs as i32).encode(buf).unwrap();

        let mut long_value: u64 = 0;
        let mut bit_offset = 0;
        let mut longs_written = 0;

        for &state in &self.blocks {
            let index = palette.iter().position(|&s| s == state).unwrap() as u64;
            long_value |= (index & mask) << bit_offset;
            bit_offset += bits_per_entry as usize;

            if bit_offset >= 64 {
                (long_value as i64).encode(buf).unwrap();
                longs_written += 1;
                long_value = 0;
                bit_offset = 0;
            }
        }

        // Write remaining partial long
        if bit_offset > 0 {
            (long_value as i64).encode(buf).unwrap();
            longs_written += 1;
        }

        // Pad with zeros if needed (shouldn't happen with correct math)
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
