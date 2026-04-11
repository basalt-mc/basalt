use crate::{Decode, Encode, EncodedSize, Error, Result};

const SEGMENT_BITS: u8 = 0x7F;
const CONTINUE_BIT: u8 = 0x80;

/// Variable-length i32, encoded in 1-5 bytes.
///
/// MSB of each byte is the continuation bit. Lower 7 bits carry the value,
/// least significant group first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VarInt(pub i32);

impl VarInt {
    /// Maximum number of bytes a VarInt can occupy.
    pub const MAX_BYTES: usize = 5;
}

impl Encode for VarInt {
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        let mut value = self.0 as u32;
        loop {
            if value & !(SEGMENT_BITS as u32) == 0 {
                buf.push(value as u8);
                return Ok(());
            }
            buf.push((value as u8 & SEGMENT_BITS) | CONTINUE_BIT);
            value >>= 7;
        }
    }
}

impl Decode for VarInt {
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        let mut value: u32 = 0;
        let mut position: u32 = 0;
        let start = *buf;

        loop {
            if buf.is_empty() {
                return Err(Error::BufferUnderflow {
                    needed: 1,
                    available: 0,
                });
            }

            let byte = buf[0];
            *buf = &buf[1..];

            value |= ((byte & SEGMENT_BITS) as u32) << position;
            position += 7;

            if byte & CONTINUE_BIT == 0 {
                return Ok(VarInt(value as i32));
            }

            if position >= 32 {
                // Reset cursor to start for accurate error reporting
                *buf = start;
                return Err(Error::VarIntTooLarge);
            }
        }
    }
}

impl EncodedSize for VarInt {
    fn encoded_size(&self) -> usize {
        let value = self.0 as u32;
        match value {
            0..=0x7F => 1,
            0x80..=0x3FFF => 2,
            0x4000..=0x1FFFFF => 3,
            0x200000..=0xFFFFFFF => 4,
            _ => 5,
        }
    }
}

impl From<i32> for VarInt {
    fn from(value: i32) -> Self {
        VarInt(value)
    }
}

impl From<VarInt> for i32 {
    fn from(value: VarInt) -> Self {
        value.0
    }
}

/// Variable-length i64, encoded in 1-10 bytes.
///
/// Same encoding as VarInt but for 64-bit values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VarLong(pub i64);

impl VarLong {
    /// Maximum number of bytes a VarLong can occupy.
    pub const MAX_BYTES: usize = 10;
}

impl Encode for VarLong {
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        let mut value = self.0 as u64;
        loop {
            if value & !(SEGMENT_BITS as u64) == 0 {
                buf.push(value as u8);
                return Ok(());
            }
            buf.push((value as u8 & SEGMENT_BITS) | CONTINUE_BIT);
            value >>= 7;
        }
    }
}

impl Decode for VarLong {
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        let mut value: u64 = 0;
        let mut position: u32 = 0;
        let start = *buf;

        loop {
            if buf.is_empty() {
                return Err(Error::BufferUnderflow {
                    needed: 1,
                    available: 0,
                });
            }

            let byte = buf[0];
            *buf = &buf[1..];

            value |= ((byte & SEGMENT_BITS) as u64) << position;
            position += 7;

            if byte & CONTINUE_BIT == 0 {
                return Ok(VarLong(value as i64));
            }

            if position >= 64 {
                *buf = start;
                return Err(Error::VarIntTooLarge);
            }
        }
    }
}

impl EncodedSize for VarLong {
    fn encoded_size(&self) -> usize {
        let value = self.0 as u64;
        match value {
            0..=0x7F => 1,
            0x80..=0x3FFF => 2,
            0x4000..=0x1FFFFF => 3,
            0x200000..=0xFFFFFFF => 4,
            0x10000000..=0x7FFFFFFFF => 5,
            0x800000000..=0x3FFFFFFFFFF => 6,
            0x40000000000..=0x1FFFFFFFFFFFF => 7,
            0x2000000000000..=0xFFFFFFFFFFFFFF => 8,
            0x100000000000000..=0x7FFFFFFFFFFFFFFF => 9,
            _ => 10,
        }
    }
}

impl From<i64> for VarLong {
    fn from(value: i64) -> Self {
        VarLong(value)
    }
}

impl From<VarLong> for i64 {
    fn from(value: VarLong) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip_varint(value: i32) {
        let var = VarInt(value);
        let mut buf = Vec::with_capacity(var.encoded_size());
        var.encode(&mut buf).unwrap();
        assert_eq!(buf.len(), var.encoded_size());

        let mut cursor = buf.as_slice();
        let decoded = VarInt::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded.0, value);
    }

    fn roundtrip_varlong(value: i64) {
        let var = VarLong(value);
        let mut buf = Vec::with_capacity(var.encoded_size());
        var.encode(&mut buf).unwrap();
        assert_eq!(buf.len(), var.encoded_size());

        let mut cursor = buf.as_slice();
        let decoded = VarLong::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded.0, value);
    }

    // -- VarInt known values (from wiki.vg) --

    #[test]
    fn varint_zero() {
        let mut buf = Vec::new();
        VarInt(0).encode(&mut buf).unwrap();
        assert_eq!(buf, [0x00]);
        roundtrip_varint(0);
    }

    #[test]
    fn varint_one() {
        let mut buf = Vec::new();
        VarInt(1).encode(&mut buf).unwrap();
        assert_eq!(buf, [0x01]);
        roundtrip_varint(1);
    }

    #[test]
    fn varint_127() {
        let mut buf = Vec::new();
        VarInt(127).encode(&mut buf).unwrap();
        assert_eq!(buf, [0x7F]);
        roundtrip_varint(127);
    }

    #[test]
    fn varint_128() {
        let mut buf = Vec::new();
        VarInt(128).encode(&mut buf).unwrap();
        assert_eq!(buf, [0x80, 0x01]);
        roundtrip_varint(128);
    }

    #[test]
    fn varint_255() {
        let mut buf = Vec::new();
        VarInt(255).encode(&mut buf).unwrap();
        assert_eq!(buf, [0xFF, 0x01]);
        roundtrip_varint(255);
    }

    #[test]
    fn varint_25565() {
        let mut buf = Vec::new();
        VarInt(25565).encode(&mut buf).unwrap();
        assert_eq!(buf, [0xDD, 0xC7, 0x01]);
        roundtrip_varint(25565);
    }

    #[test]
    fn varint_max() {
        let mut buf = Vec::new();
        VarInt(i32::MAX).encode(&mut buf).unwrap();
        assert_eq!(buf, [0xFF, 0xFF, 0xFF, 0xFF, 0x07]);
        roundtrip_varint(i32::MAX);
    }

    #[test]
    fn varint_minus_one() {
        let mut buf = Vec::new();
        VarInt(-1).encode(&mut buf).unwrap();
        assert_eq!(buf, [0xFF, 0xFF, 0xFF, 0xFF, 0x0F]);
        roundtrip_varint(-1);
    }

    #[test]
    fn varint_min() {
        let mut buf = Vec::new();
        VarInt(i32::MIN).encode(&mut buf).unwrap();
        assert_eq!(buf, [0x80, 0x80, 0x80, 0x80, 0x08]);
        roundtrip_varint(i32::MIN);
    }

    // -- VarInt errors --

    #[test]
    fn varint_empty_buffer() {
        let mut cursor: &[u8] = &[];
        assert!(matches!(
            VarInt::decode(&mut cursor),
            Err(Error::BufferUnderflow { .. })
        ));
    }

    #[test]
    fn varint_too_large() {
        // 6 continuation bytes — exceeds 5-byte limit
        let mut cursor: &[u8] = &[0x80, 0x80, 0x80, 0x80, 0x80, 0x01];
        assert!(matches!(
            VarInt::decode(&mut cursor),
            Err(Error::VarIntTooLarge)
        ));
    }

    #[test]
    fn varint_truncated() {
        // Continuation bit set but no next byte
        let mut cursor: &[u8] = &[0x80];
        assert!(matches!(
            VarInt::decode(&mut cursor),
            Err(Error::BufferUnderflow { .. })
        ));
    }

    // -- VarInt encoded_size --

    #[test]
    fn varint_encoded_size() {
        assert_eq!(VarInt(0).encoded_size(), 1);
        assert_eq!(VarInt(127).encoded_size(), 1);
        assert_eq!(VarInt(128).encoded_size(), 2);
        assert_eq!(VarInt(16383).encoded_size(), 2);
        assert_eq!(VarInt(16384).encoded_size(), 3);
        assert_eq!(VarInt(i32::MAX).encoded_size(), 5);
        assert_eq!(VarInt(-1).encoded_size(), 5);
        assert_eq!(VarInt(i32::MIN).encoded_size(), 5);
    }

    // -- VarInt conversions --

    #[test]
    fn varint_from_i32() {
        let v: VarInt = 42.into();
        assert_eq!(v.0, 42);
    }

    #[test]
    fn varint_into_i32() {
        let v: i32 = VarInt(42).into();
        assert_eq!(v, 42);
    }

    // -- VarLong known values --

    #[test]
    fn varlong_zero() {
        roundtrip_varlong(0);
    }

    #[test]
    fn varlong_one() {
        roundtrip_varlong(1);
    }

    #[test]
    fn varlong_max() {
        roundtrip_varlong(i64::MAX);
    }

    #[test]
    fn varlong_min() {
        roundtrip_varlong(i64::MIN);
    }

    #[test]
    fn varlong_minus_one() {
        roundtrip_varlong(-1);
    }

    // -- VarLong errors --

    #[test]
    fn varlong_empty_buffer() {
        let mut cursor: &[u8] = &[];
        assert!(matches!(
            VarLong::decode(&mut cursor),
            Err(Error::BufferUnderflow { .. })
        ));
    }

    #[test]
    fn varlong_too_large() {
        // 11 continuation bytes — exceeds 10-byte limit
        let mut cursor: &[u8] = &[0x80; 11];
        assert!(matches!(
            VarLong::decode(&mut cursor),
            Err(Error::VarIntTooLarge)
        ));
    }

    // -- VarLong encoded_size --

    #[test]
    fn varlong_encoded_size() {
        assert_eq!(VarLong(0).encoded_size(), 1);
        assert_eq!(VarLong(127).encoded_size(), 1);
        assert_eq!(VarLong(128).encoded_size(), 2);
        assert_eq!(VarLong(i64::MAX).encoded_size(), 9);
        assert_eq!(VarLong(-1).encoded_size(), 10);
        assert_eq!(VarLong(i64::MIN).encoded_size(), 10);
    }

    // -- VarLong conversions --

    #[test]
    fn varlong_from_i64() {
        let v: VarLong = 42i64.into();
        assert_eq!(v.0, 42);
    }

    #[test]
    fn varlong_into_i64() {
        let v: i64 = VarLong(42).into();
        assert_eq!(v, 42);
    }

    // -- proptest --

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn varint_roundtrip(v: i32) {
                roundtrip_varint(v);
            }

            #[test]
            fn varlong_roundtrip(v: i64) {
                roundtrip_varlong(v);
            }
        }
    }
}
