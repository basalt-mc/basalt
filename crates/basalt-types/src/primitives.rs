use crate::{Decode, Encode, EncodedSize, Error, Result};

/// Encodes a boolean as a single byte in the Minecraft protocol.
///
/// The Minecraft protocol represents booleans as a single unsigned byte:
/// `0x00` for `false`, `0x01` for `true`. This is used in many packets
/// for flags like on-ground state, sneaking, sprinting, etc.
impl Encode for bool {
    /// Writes `0x01` if true, `0x00` if false. Always writes exactly one byte.
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        buf.push(if *self { 0x01 } else { 0x00 });
        Ok(())
    }
}

/// Decodes a boolean from a single byte in the Minecraft protocol.
///
/// Any non-zero byte is interpreted as `true`, matching the Minecraft
/// server behavior. This is intentionally lenient — the protocol spec
/// says `0x01` for true, but servers may send other non-zero values.
impl Decode for bool {
    /// Reads one byte. Returns `true` for any non-zero value, `false` for `0x00`.
    ///
    /// Fails with `BufferUnderflow` if the buffer is empty.
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        if buf.is_empty() {
            return Err(Error::BufferUnderflow {
                needed: 1,
                available: 0,
            });
        }
        let value = buf[0] != 0;
        *buf = &buf[1..];
        Ok(value)
    }
}

/// A boolean always occupies exactly one byte on the wire.
impl EncodedSize for bool {
    fn encoded_size(&self) -> usize {
        1
    }
}

/// Generates `Encode`, `Decode`, and `EncodedSize` implementations for
/// fixed-size numeric types using big-endian byte order.
///
/// The Minecraft protocol uses big-endian (network byte order) for all
/// fixed-size integers and floating-point values. Each type occupies a
/// fixed number of bytes on the wire, regardless of the value.
macro_rules! impl_numeric {
    ($ty:ty, $size:expr) => {
        /// Encodes as a fixed-size big-endian value.
        ///
        /// The Minecraft protocol uses big-endian (network byte order) for all
        /// fixed-size numeric types. The value is written as exactly
        #[doc = concat!(stringify!($size), " bytes.")]
        impl Encode for $ty {
            /// Writes the value as big-endian bytes. Always writes exactly
            #[doc = concat!(stringify!($size), " bytes.")]
            fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
                buf.extend_from_slice(&self.to_be_bytes());
                Ok(())
            }
        }

        /// Decodes from a fixed-size big-endian value.
        ///
        /// Reads exactly
        #[doc = concat!(stringify!($size), " bytes from the buffer and interprets them as big-endian.")]
        impl Decode for $ty {
            /// Reads
            #[doc = concat!(stringify!($size), " big-endian bytes and advances the cursor.")]
            ///
            /// Fails with `BufferUnderflow` if fewer than
            #[doc = concat!(stringify!($size), " bytes remain.")]
            fn decode(buf: &mut &[u8]) -> Result<Self> {
                if buf.len() < $size {
                    return Err(Error::BufferUnderflow {
                        needed: $size,
                        available: buf.len(),
                    });
                }
                let (bytes, rest) = buf.split_at($size);
                let value = <$ty>::from_be_bytes(bytes.try_into().unwrap());
                *buf = rest;
                Ok(value)
            }
        }

        /// The encoded size is always
        #[doc = concat!(stringify!($size), " bytes, regardless of the value.")]
        impl EncodedSize for $ty {
            fn encoded_size(&self) -> usize {
                $size
            }
        }
    };
}

impl_numeric!(u8, 1);
impl_numeric!(u16, 2);
impl_numeric!(u32, 4);
impl_numeric!(u64, 8);
impl_numeric!(i8, 1);
impl_numeric!(i16, 2);
impl_numeric!(i32, 4);
impl_numeric!(i64, 8);
impl_numeric!(f32, 4);
impl_numeric!(f64, 8);

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: encode a value, then decode it and verify roundtrip.
    fn roundtrip<T: Encode + Decode + EncodedSize + PartialEq + std::fmt::Debug>(value: T) {
        let mut buf = Vec::with_capacity(value.encoded_size());
        value.encode(&mut buf).unwrap();
        assert_eq!(buf.len(), value.encoded_size());

        let mut cursor = buf.as_slice();
        let decoded = T::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, value);
    }

    /// Helper: verify decode fails on a too-short buffer.
    fn decode_underflow<T: Decode + std::fmt::Debug>(short_buf: &[u8]) {
        let mut cursor = short_buf;
        let result = T::decode(&mut cursor);
        assert!(matches!(result, Err(Error::BufferUnderflow { .. })));
    }

    // -- bool --

    #[test]
    fn bool_true() {
        roundtrip(true);
    }

    #[test]
    fn bool_false() {
        roundtrip(false);
    }

    #[test]
    fn bool_nonzero_is_true() {
        let mut cursor: &[u8] = &[0x42];
        assert!(bool::decode(&mut cursor).unwrap());
    }

    #[test]
    fn bool_underflow() {
        decode_underflow::<bool>(&[]);
    }

    // -- u8 / i8 --

    #[test]
    fn u8_roundtrip() {
        roundtrip(0u8);
        roundtrip(u8::MAX);
    }

    #[test]
    fn i8_roundtrip() {
        roundtrip(0i8);
        roundtrip(i8::MAX);
        roundtrip(i8::MIN);
    }

    // -- u16 / i16 --

    #[test]
    fn u16_roundtrip() {
        roundtrip(0u16);
        roundtrip(u16::MAX);
    }

    #[test]
    fn u16_big_endian() {
        let mut buf = Vec::new();
        0x0102u16.encode(&mut buf).unwrap();
        assert_eq!(buf, [0x01, 0x02]);
    }

    #[test]
    fn i16_roundtrip() {
        roundtrip(0i16);
        roundtrip(i16::MAX);
        roundtrip(i16::MIN);
    }

    #[test]
    fn u16_underflow() {
        decode_underflow::<u16>(&[0x01]);
    }

    // -- u32 / i32 --

    #[test]
    fn u32_roundtrip() {
        roundtrip(0u32);
        roundtrip(u32::MAX);
    }

    #[test]
    fn u32_big_endian() {
        let mut buf = Vec::new();
        0x01020304u32.encode(&mut buf).unwrap();
        assert_eq!(buf, [0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn i32_roundtrip() {
        roundtrip(0i32);
        roundtrip(i32::MAX);
        roundtrip(i32::MIN);
    }

    #[test]
    fn u32_underflow() {
        decode_underflow::<u32>(&[0x01, 0x02, 0x03]);
    }

    // -- u64 / i64 --

    #[test]
    fn u64_roundtrip() {
        roundtrip(0u64);
        roundtrip(u64::MAX);
    }

    #[test]
    fn i64_roundtrip() {
        roundtrip(0i64);
        roundtrip(i64::MAX);
        roundtrip(i64::MIN);
    }

    #[test]
    fn u64_underflow() {
        decode_underflow::<u64>(&[0x01; 7]);
    }

    // -- f32 / f64 --

    #[test]
    fn f32_roundtrip() {
        roundtrip(0.0f32);
        roundtrip(f32::MAX);
        roundtrip(f32::MIN);
        roundtrip(f32::INFINITY);
        roundtrip(f32::NEG_INFINITY);
    }

    #[test]
    fn f32_nan() {
        let mut buf = Vec::new();
        f32::NAN.encode(&mut buf).unwrap();
        let mut cursor = buf.as_slice();
        let decoded = f32::decode(&mut cursor).unwrap();
        assert!(decoded.is_nan());
    }

    #[test]
    fn f64_roundtrip() {
        roundtrip(0.0f64);
        roundtrip(f64::MAX);
        roundtrip(f64::MIN);
        roundtrip(f64::INFINITY);
        roundtrip(f64::NEG_INFINITY);
    }

    #[test]
    fn f64_nan() {
        let mut buf = Vec::new();
        f64::NAN.encode(&mut buf).unwrap();
        let mut cursor = buf.as_slice();
        let decoded = f64::decode(&mut cursor).unwrap();
        assert!(decoded.is_nan());
    }

    #[test]
    fn f64_underflow() {
        decode_underflow::<f64>(&[0x01; 7]);
    }

    // -- proptest --

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn bool_roundtrip(v: bool) {
                roundtrip(v);
            }

            #[test]
            fn u8_roundtrip(v: u8) {
                roundtrip(v);
            }

            #[test]
            fn i8_roundtrip(v: i8) {
                roundtrip(v);
            }

            #[test]
            fn u16_roundtrip(v: u16) {
                roundtrip(v);
            }

            #[test]
            fn i16_roundtrip(v: i16) {
                roundtrip(v);
            }

            #[test]
            fn u32_roundtrip(v: u32) {
                roundtrip(v);
            }

            #[test]
            fn i32_roundtrip(v: i32) {
                roundtrip(v);
            }

            #[test]
            fn u64_roundtrip(v: u64) {
                roundtrip(v);
            }

            #[test]
            fn i64_roundtrip(v: i64) {
                roundtrip(v);
            }

            #[test]
            fn f32_roundtrip_finite(v in proptest::num::f32::NORMAL | proptest::num::f32::SUBNORMAL | proptest::num::f32::ZERO) {
                roundtrip(v);
            }

            #[test]
            fn f64_roundtrip_finite(v in proptest::num::f64::NORMAL | proptest::num::f64::SUBNORMAL | proptest::num::f64::ZERO) {
                roundtrip(v);
            }
        }
    }
}
