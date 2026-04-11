use crate::{Decode, Encode, EncodedSize, Result};

/// A packed block position encoded as a single 64-bit integer.
///
/// The Minecraft protocol packs three spatial coordinates into one `i64`
/// to minimize bandwidth for the most common spatial reference in the game.
/// Used in packets like block changes, player digging, block placement,
/// sign updates, and many more.
///
/// Bit layout (MSB to LSB):
/// - x: 26 bits (signed, range -33554432 to 33554431)
/// - z: 26 bits (signed, range -33554432 to 33554431)
/// - y: 12 bits (signed, range -2048 to 2047)
///
/// Encoding: `(x & 0x3FFFFFF) << 38 | (z & 0x3FFFFFF) << 12 | (y & 0xFFF)`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Position {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Position {
    /// Creates a new position from block coordinates.
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// Packs the position into a single 64-bit integer.
    ///
    /// The x and z coordinates are masked to 26 bits, y to 12 bits.
    /// Bits beyond the valid range are silently truncated.
    fn pack(&self) -> i64 {
        ((self.x as i64 & 0x3FFFFFF) << 38)
            | ((self.z as i64 & 0x3FFFFFF) << 12)
            | (self.y as i64 & 0xFFF)
    }

    /// Unpacks a 64-bit integer into x, y, z coordinates.
    ///
    /// Performs sign extension for each field: x and z from 26 bits,
    /// y from 12 bits. This correctly handles negative coordinates.
    fn unpack(val: i64) -> Self {
        let mut x = (val >> 38) as i32;
        let mut z = ((val >> 12) & 0x3FFFFFF) as i32;
        let mut y = (val & 0xFFF) as i32;

        // Sign-extend from 26 bits for x and z
        if x >= 1 << 25 {
            x -= 1 << 26;
        }
        if z >= 1 << 25 {
            z -= 1 << 26;
        }
        // Sign-extend from 12 bits for y
        if y >= 1 << 11 {
            y -= 1 << 12;
        }

        Self { x, y, z }
    }
}

/// Encodes a Position as a packed 64-bit big-endian integer.
///
/// The three coordinates are packed into a single `i64` using the bit
/// layout described on the type. The packed value is then written as
/// 8 big-endian bytes, matching the Minecraft protocol wire format.
impl Encode for Position {
    /// Packs x, y, z into a 64-bit integer and writes it as 8 big-endian bytes.
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        self.pack().encode(buf)
    }
}

/// Decodes a Position from a packed 64-bit big-endian integer.
///
/// Reads 8 bytes as a big-endian `i64`, then unpacks x (26 bits),
/// z (26 bits), and y (12 bits) with proper sign extension for
/// negative coordinates.
impl Decode for Position {
    /// Reads 8 big-endian bytes, unpacks into x, y, z with sign extension.
    ///
    /// Fails with `Error::BufferUnderflow` if fewer than 8 bytes remain.
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        let val = i64::decode(buf)?;
        Ok(Self::unpack(val))
    }
}

/// A Position always occupies exactly 8 bytes on the wire (one packed i64).
impl EncodedSize for Position {
    fn encoded_size(&self) -> usize {
        8
    }
}

/// A block position using full 32-bit coordinates.
///
/// BlockPosition represents a block's location in the world without the bit
/// packing constraints of [`Position`]. It is used internally for world
/// logic and converts to/from `Position` for protocol serialization.
///
/// Unlike `Position`, BlockPosition is not directly serialized on the wire —
/// it converts through `Position` for encoding. This type exists to
/// provide a more ergonomic API for working with block coordinates
/// without worrying about bit packing limitations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockPosition {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPosition {
    /// Creates a new block position.
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// Returns the chunk position that contains this block.
    ///
    /// Chunk coordinates are derived by dividing block coordinates by 16
    /// (arithmetic right shift by 4). This matches the Minecraft convention
    /// where each chunk is a 16x16 column of blocks.
    pub fn chunk_pos(&self) -> ChunkPosition {
        ChunkPosition {
            x: self.x >> 4,
            z: self.z >> 4,
        }
    }
}

/// Converts a [`Position`] (packed wire format) into a [`BlockPosition`] (full i32 coordinates).
impl From<Position> for BlockPosition {
    fn from(pos: Position) -> Self {
        Self {
            x: pos.x,
            y: pos.y,
            z: pos.z,
        }
    }
}

/// Converts a [`BlockPosition`] into a [`Position`] for wire serialization.
///
/// Coordinates outside the Position range (x/z: 26-bit signed, y: 12-bit
/// signed) will be silently truncated during packing.
impl From<BlockPosition> for Position {
    fn from(pos: BlockPosition) -> Self {
        Position {
            x: pos.x,
            y: pos.y,
            z: pos.z,
        }
    }
}

/// A chunk position in the world, identified by chunk-level x and z coordinates.
///
/// Each chunk is a 16x16 column of blocks. ChunkPosition represents the chunk's
/// location in the world grid. It is derived from block coordinates by
/// dividing by 16. Used for chunk loading, unloading, and spatial indexing.
///
/// ChunkPosition is not directly serialized on the wire — chunk packets use
/// their own field encoding. This type exists for spatial logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkPosition {
    pub x: i32,
    pub z: i32,
}

impl ChunkPosition {
    /// Creates a new chunk position.
    pub fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }
}

/// Converts a [`BlockPosition`] to the [`ChunkPosition`] that contains it.
impl From<BlockPosition> for ChunkPosition {
    fn from(pos: BlockPosition) -> Self {
        pos.chunk_pos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    fn roundtrip(x: i32, y: i32, z: i32) {
        let pos = Position::new(x, y, z);
        let mut buf = Vec::with_capacity(pos.encoded_size());
        pos.encode(&mut buf).unwrap();
        assert_eq!(buf.len(), 8);

        let mut cursor = buf.as_slice();
        let decoded = Position::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, pos);
    }

    // -- Position encode/decode --

    #[test]
    fn origin() {
        roundtrip(0, 0, 0);
    }

    #[test]
    fn positive_coords() {
        roundtrip(100, 64, 200);
    }

    #[test]
    fn negative_coords() {
        roundtrip(-100, -32, -200);
    }

    #[test]
    fn max_values() {
        // x/z max: 2^25 - 1 = 33554431, y max: 2^11 - 1 = 2047
        roundtrip(33554431, 2047, 33554431);
    }

    #[test]
    fn min_values() {
        // x/z min: -2^25 = -33554432, y min: -2^11 = -2048
        roundtrip(-33554432, -2048, -33554432);
    }

    #[test]
    fn typical_overworld() {
        // Typical overworld spawn area
        roundtrip(256, 72, -128);
    }

    #[test]
    fn position_underflow() {
        let mut cursor: &[u8] = &[0x01; 7];
        assert!(matches!(
            Position::decode(&mut cursor),
            Err(Error::BufferUnderflow { .. })
        ));
    }

    #[test]
    fn encoded_size_is_8() {
        assert_eq!(Position::new(0, 0, 0).encoded_size(), 8);
        assert_eq!(Position::new(100, 200, 300).encoded_size(), 8);
    }

    // -- Pack/unpack --

    #[test]
    fn pack_known_value() {
        // From wiki.vg: position (18357644, 831, -20882616)
        let pos = Position::new(18357644, 831, -20882616);
        let packed = pos.pack();
        let unpacked = Position::unpack(packed);
        assert_eq!(unpacked, pos);
    }

    // -- BlockPosition --

    #[test]
    fn blockpos_to_position() {
        let bp = BlockPosition::new(100, 64, -200);
        let pos: Position = bp.into();
        assert_eq!(pos, Position::new(100, 64, -200));
    }

    #[test]
    fn position_to_blockpos() {
        let pos = Position::new(-50, 128, 300);
        let bp: BlockPosition = pos.into();
        assert_eq!(bp, BlockPosition::new(-50, 128, 300));
    }

    // -- ChunkPosition --

    #[test]
    fn blockpos_to_chunkpos() {
        let bp = BlockPosition::new(100, 64, -200);
        let cp = bp.chunk_pos();
        assert_eq!(cp, ChunkPosition::new(6, -13));
    }

    #[test]
    fn blockpos_to_chunkpos_negative() {
        // Block (-1, 0, -1) should be in chunk (-1, -1)
        let bp = BlockPosition::new(-1, 0, -1);
        let cp = bp.chunk_pos();
        assert_eq!(cp, ChunkPosition::new(-1, -1));
    }

    #[test]
    fn blockpos_to_chunkpos_origin() {
        let bp = BlockPosition::new(0, 0, 0);
        let cp = bp.chunk_pos();
        assert_eq!(cp, ChunkPosition::new(0, 0));
    }

    #[test]
    fn chunkpos_from_blockpos() {
        let bp = BlockPosition::new(32, 0, 48);
        let cp: ChunkPosition = bp.into();
        assert_eq!(cp, ChunkPosition::new(2, 3));
    }

    // -- proptest --

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn position_roundtrip(
                x in -33554432i32..=33554431,
                y in -2048i32..=2047,
                z in -33554432i32..=33554431,
            ) {
                roundtrip(x, y, z);
            }
        }
    }
}
