use std::fmt;

use crate::{Decode, Encode, EncodedSize, Result};

/// A 128-bit universally unique identifier used throughout the Minecraft protocol.
///
/// UUIDs identify players, entities, and various protocol objects. They are
/// encoded as two consecutive big-endian 64-bit integers (most significant
/// bits first, then least significant bits), occupying exactly 16 bytes on
/// the wire. The Minecraft protocol uses UUIDs in login packets (player UUID),
/// entity spawn packets, player info, and boss bar management.
///
/// The standard display format is `8-4-4-4-12` lowercase hex with dashes
/// (e.g., `550e8400-e29b-41d4-a716-446655440000`).
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Uuid {
    /// Most significant 64 bits of the UUID.
    pub most: u64,
    /// Least significant 64 bits of the UUID.
    pub least: u64,
}

impl Uuid {
    /// Creates a new UUID from its most and least significant 64-bit halves.
    pub fn new(most: u64, least: u64) -> Self {
        Self { most, least }
    }

    /// Creates a UUID from a 16-byte array in big-endian order.
    ///
    /// The first 8 bytes form the most significant bits, the last 8 form
    /// the least significant bits.
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        let most = u64::from_be_bytes(bytes[..8].try_into().unwrap());
        let least = u64::from_be_bytes(bytes[8..].try_into().unwrap());
        Self { most, least }
    }

    /// Converts the UUID to a 16-byte array in big-endian order.
    pub fn to_bytes(self) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&self.most.to_be_bytes());
        bytes[8..].copy_from_slice(&self.least.to_be_bytes());
        bytes
    }
}

/// Encodes a UUID as two consecutive big-endian u64 values (16 bytes total).
///
/// The most significant 64 bits are written first, followed by the least
/// significant 64 bits. This matches the Minecraft protocol wire format
/// for all UUID fields.
impl Encode for Uuid {
    /// Writes the UUID as 16 big-endian bytes (most significant first).
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        self.most.encode(buf)?;
        self.least.encode(buf)
    }
}

/// Decodes a UUID from two consecutive big-endian u64 values (16 bytes).
///
/// Reads the most significant 64 bits first, then the least significant
/// 64 bits. Fails if fewer than 16 bytes remain in the buffer.
impl Decode for Uuid {
    /// Reads 16 big-endian bytes and reconstructs the UUID.
    ///
    /// Fails with `Error::BufferUnderflow` if fewer than 16 bytes remain.
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        let most = u64::decode(buf)?;
        let least = u64::decode(buf)?;
        Ok(Self { most, least })
    }
}

/// A UUID always occupies exactly 16 bytes on the wire.
impl EncodedSize for Uuid {
    fn encoded_size(&self) -> usize {
        16
    }
}

/// Displays the UUID in the standard `8-4-4-4-12` lowercase hex format.
///
/// This matches the format used by Mojang's API and the Minecraft client
/// (e.g., `550e8400-e29b-41d4-a716-446655440000`).
impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.to_bytes();
        write!(
            f,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            bytes[0],
            bytes[1],
            bytes[2],
            bytes[3],
            bytes[4],
            bytes[5],
            bytes[6],
            bytes[7],
            bytes[8],
            bytes[9],
            bytes[10],
            bytes[11],
            bytes[12],
            bytes[13],
            bytes[14],
            bytes[15],
        )
    }
}

/// Debug output uses the same `8-4-4-4-12` hex format as Display for readability.
impl fmt::Debug for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Uuid({})", self)
    }
}

/// Converts a 16-byte array into a UUID.
impl From<[u8; 16]> for Uuid {
    fn from(bytes: [u8; 16]) -> Self {
        Self::from_bytes(bytes)
    }
}

/// Converts a UUID into a 16-byte array in big-endian order.
impl From<Uuid> for [u8; 16] {
    fn from(uuid: Uuid) -> Self {
        uuid.to_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(most: u64, least: u64) {
        let uuid = Uuid::new(most, least);
        let mut buf = Vec::with_capacity(uuid.encoded_size());
        uuid.encode(&mut buf).unwrap();
        assert_eq!(buf.len(), 16);

        let mut cursor = buf.as_slice();
        let decoded = Uuid::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, uuid);
    }

    #[test]
    fn zero_uuid() {
        roundtrip(0, 0);
    }

    #[test]
    fn max_uuid() {
        roundtrip(u64::MAX, u64::MAX);
    }

    #[test]
    fn known_uuid() {
        // Notch's UUID: 069a79f4-44e9-4726-a5be-fca90e38aaf5
        let uuid = Uuid::new(0x069a79f444e94726, 0xa5befca90e38aaf5);
        roundtrip(uuid.most, uuid.least);
    }

    #[test]
    fn display_format() {
        let uuid = Uuid::new(0x550e8400e29b41d4, 0xa716446655440000);
        assert_eq!(uuid.to_string(), "550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn debug_format() {
        let uuid = Uuid::new(0x550e8400e29b41d4, 0xa716446655440000);
        assert_eq!(
            format!("{:?}", uuid),
            "Uuid(550e8400-e29b-41d4-a716-446655440000)"
        );
    }

    #[test]
    fn from_bytes() {
        let bytes = [
            0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44,
            0x00, 0x00,
        ];
        let uuid = Uuid::from_bytes(bytes);
        assert_eq!(uuid.most, 0x550e8400e29b41d4);
        assert_eq!(uuid.least, 0xa716446655440000);
    }

    #[test]
    fn to_bytes() {
        let uuid = Uuid::new(0x550e8400e29b41d4, 0xa716446655440000);
        let bytes = uuid.to_bytes();
        assert_eq!(
            bytes,
            [
                0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44,
                0x00, 0x00
            ]
        );
    }

    #[test]
    fn bytes_roundtrip() {
        let original = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let uuid = Uuid::from(original);
        let back: [u8; 16] = uuid.into();
        assert_eq!(back, original);
    }

    #[test]
    fn encoded_size_is_16() {
        assert_eq!(Uuid::new(0, 0).encoded_size(), 16);
    }

    #[test]
    fn underflow() {
        let mut cursor: &[u8] = &[0x01; 15];
        assert!(Uuid::decode(&mut cursor).is_err());
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn uuid_roundtrip(most: u64, least: u64) {
                roundtrip(most, least);
            }
        }
    }
}
