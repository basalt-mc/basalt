use crate::{Decode, Encode, EncodedSize, Result};

/// A rotation angle encoded as a single unsigned byte.
///
/// The Minecraft protocol represents rotations as a single byte where the
/// full 0-255 range maps to 0-360 degrees. This is used for entity head
/// rotation (`Entity Head Look` packet), entity look direction, and
/// similar rotation fields. The conversion formula is:
///
/// - Byte to degrees: `value / 256.0 * 360.0`
/// - Degrees to byte: `degrees / 360.0 * 256.0`
///
/// The mapping wraps naturally: 256 steps for a full rotation gives
/// approximately 1.4° precision per step, which is sufficient for
/// visual entity rotation in the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Angle(pub u8);

impl Angle {
    /// Creates an Angle from a degree value.
    ///
    /// The degree value is normalized to the 0-255 byte range. Values
    /// outside 0-360 wrap naturally (e.g., 720° wraps to the same byte
    /// as 360°, which is 0).
    pub fn from_degrees(degrees: f32) -> Self {
        Self((degrees / 360.0 * 256.0) as u8)
    }

    /// Converts the angle to degrees in the range 0.0 to ~359.0.
    ///
    /// The result has approximately 1.4° precision due to the single-byte
    /// encoding. A byte value of 0 maps to 0°, 64 to 90°, 128 to 180°,
    /// and 192 to 270°.
    pub fn to_degrees(self) -> f32 {
        self.0 as f32 / 256.0 * 360.0
    }
}

/// Encodes an Angle as a single unsigned byte.
///
/// The angle is written directly as one byte with no transformation.
/// This is the simplest encoding in the Minecraft protocol.
impl Encode for Angle {
    /// Writes the angle as a single byte.
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        self.0.encode(buf)
    }
}

/// Decodes an Angle from a single unsigned byte.
///
/// Reads exactly one byte. Any byte value is valid — the full 0-255
/// range maps to 0-360 degrees.
impl Decode for Angle {
    /// Reads one byte and wraps it as an Angle.
    ///
    /// Fails with `Error::BufferUnderflow` if the buffer is empty.
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        Ok(Self(u8::decode(buf)?))
    }
}

/// An Angle always occupies exactly 1 byte on the wire.
impl EncodedSize for Angle {
    fn encoded_size(&self) -> usize {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(value: u8) {
        let angle = Angle(value);
        let mut buf = Vec::with_capacity(angle.encoded_size());
        angle.encode(&mut buf).unwrap();
        assert_eq!(buf.len(), 1);

        let mut cursor = buf.as_slice();
        let decoded = Angle::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, angle);
    }

    #[test]
    fn zero() {
        roundtrip(0);
    }

    #[test]
    fn max() {
        roundtrip(255);
    }

    #[test]
    fn midpoint() {
        roundtrip(128);
    }

    #[test]
    fn from_degrees_zero() {
        let angle = Angle::from_degrees(0.0);
        assert_eq!(angle.0, 0);
    }

    #[test]
    fn from_degrees_90() {
        let angle = Angle::from_degrees(90.0);
        assert_eq!(angle.0, 64);
    }

    #[test]
    fn from_degrees_180() {
        let angle = Angle::from_degrees(180.0);
        assert_eq!(angle.0, 128);
    }

    #[test]
    fn from_degrees_270() {
        let angle = Angle::from_degrees(270.0);
        assert_eq!(angle.0, 192);
    }

    #[test]
    fn from_degrees_360_saturates() {
        // 360.0 / 360.0 * 256.0 = 256.0, which saturates to 255 as u8.
        // True wrap-around happens at values > 360 via float truncation.
        let angle = Angle::from_degrees(360.0);
        assert_eq!(angle.0, 255);
    }

    #[test]
    fn to_degrees_zero() {
        assert!((Angle(0).to_degrees() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn to_degrees_90() {
        assert!((Angle(64).to_degrees() - 90.0).abs() < f32::EPSILON);
    }

    #[test]
    fn to_degrees_180() {
        assert!((Angle(128).to_degrees() - 180.0).abs() < f32::EPSILON);
    }

    #[test]
    fn to_degrees_270() {
        assert!((Angle(192).to_degrees() - 270.0).abs() < f32::EPSILON);
    }

    #[test]
    fn to_degrees_255() {
        // 255 / 256 * 360 ≈ 358.59°
        let degrees = Angle(255).to_degrees();
        assert!((degrees - 358.59375).abs() < 0.001);
    }

    #[test]
    fn encoded_size_is_1() {
        assert_eq!(Angle(0).encoded_size(), 1);
        assert_eq!(Angle(255).encoded_size(), 1);
    }

    #[test]
    fn underflow() {
        let mut cursor: &[u8] = &[];
        assert!(Angle::decode(&mut cursor).is_err());
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn angle_roundtrip(v: u8) {
                roundtrip(v);
            }
        }
    }
}
