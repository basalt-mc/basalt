use crate::{Decode, Encode, EncodedSize, Result};

/// A 2D vector of f32 values.
///
/// Used in the Minecraft protocol for 2D positions and velocities,
/// such as player movement input and rotation deltas.
///
/// Wire format: two big-endian f32 values (x, y), 8 bytes total.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec2f {
    pub x: f32,
    pub y: f32,
}

/// Encodes a Vec2f as two consecutive big-endian f32 values.
impl Encode for Vec2f {
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        self.x.encode(buf)?;
        self.y.encode(buf)
    }
}

/// Decodes a Vec2f from two consecutive big-endian f32 values.
impl Decode for Vec2f {
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        Ok(Self {
            x: f32::decode(buf)?,
            y: f32::decode(buf)?,
        })
    }
}

/// A Vec2f always occupies 8 bytes (2 × f32).
impl EncodedSize for Vec2f {
    fn encoded_size(&self) -> usize {
        8
    }
}

/// A 3D vector of f32 values.
///
/// Used in the Minecraft protocol for positions, velocities, and
/// directions with single-precision floating point. Common in entity
/// movement and particle effects.
///
/// Wire format: three big-endian f32 values (x, y, z), 12 bytes total.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec3f {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// Encodes a Vec3f as three consecutive big-endian f32 values.
impl Encode for Vec3f {
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        self.x.encode(buf)?;
        self.y.encode(buf)?;
        self.z.encode(buf)
    }
}

/// Decodes a Vec3f from three consecutive big-endian f32 values.
impl Decode for Vec3f {
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        Ok(Self {
            x: f32::decode(buf)?,
            y: f32::decode(buf)?,
            z: f32::decode(buf)?,
        })
    }
}

/// A Vec3f always occupies 12 bytes (3 × f32).
impl EncodedSize for Vec3f {
    fn encoded_size(&self) -> usize {
        12
    }
}

/// A 3D vector of f64 values.
///
/// Used in the Minecraft protocol for precise entity positions and
/// world coordinates. Double-precision is needed because Minecraft
/// worlds can be very large (up to 30 million blocks from origin).
///
/// Wire format: three big-endian f64 values (x, y, z), 24 bytes total.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec3f64 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Encodes a Vec3f64 as three consecutive big-endian f64 values.
impl Encode for Vec3f64 {
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        self.x.encode(buf)?;
        self.y.encode(buf)?;
        self.z.encode(buf)
    }
}

/// Decodes a Vec3f64 from three consecutive big-endian f64 values.
impl Decode for Vec3f64 {
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        Ok(Self {
            x: f64::decode(buf)?,
            y: f64::decode(buf)?,
            z: f64::decode(buf)?,
        })
    }
}

/// A Vec3f64 always occupies 24 bytes (3 × f64).
impl EncodedSize for Vec3f64 {
    fn encoded_size(&self) -> usize {
        24
    }
}

/// A 3D vector of i16 values.
///
/// Used in the Minecraft protocol for relative entity movement deltas
/// in the Entity Position packet. Each unit represents 1/128 of a block.
///
/// Wire format: three big-endian i16 values (x, y, z), 6 bytes total.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Vec3i16 {
    pub x: i16,
    pub y: i16,
    pub z: i16,
}

/// Encodes a Vec3i16 as three consecutive big-endian i16 values.
impl Encode for Vec3i16 {
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        self.x.encode(buf)?;
        self.y.encode(buf)?;
        self.z.encode(buf)
    }
}

/// Decodes a Vec3i16 from three consecutive big-endian i16 values.
impl Decode for Vec3i16 {
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        Ok(Self {
            x: i16::decode(buf)?,
            y: i16::decode(buf)?,
            z: i16::decode(buf)?,
        })
    }
}

/// A Vec3i16 always occupies 6 bytes (3 × i16).
impl EncodedSize for Vec3i16 {
    fn encoded_size(&self) -> usize {
        6
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip<T: Encode + Decode + EncodedSize + PartialEq + std::fmt::Debug>(value: T) {
        let mut buf = Vec::with_capacity(value.encoded_size());
        value.encode(&mut buf).unwrap();
        assert_eq!(buf.len(), value.encoded_size());

        let mut cursor = buf.as_slice();
        let decoded = T::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, value);
    }

    #[test]
    fn vec2f_roundtrip() {
        roundtrip(Vec2f { x: 1.5, y: -2.5 });
    }

    #[test]
    fn vec2f_zero() {
        roundtrip(Vec2f::default());
    }

    #[test]
    fn vec3f_roundtrip() {
        roundtrip(Vec3f {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        });
    }

    #[test]
    fn vec3f64_roundtrip() {
        roundtrip(Vec3f64 {
            x: 100.5,
            y: 64.0,
            z: -200.25,
        });
    }

    #[test]
    fn vec3i16_roundtrip() {
        roundtrip(Vec3i16 {
            x: 100,
            y: -50,
            z: 200,
        });
    }

    #[test]
    fn encoded_sizes() {
        assert_eq!(Vec2f::default().encoded_size(), 8);
        assert_eq!(Vec3f::default().encoded_size(), 12);
        assert_eq!(Vec3f64::default().encoded_size(), 24);
        assert_eq!(Vec3i16::default().encoded_size(), 6);
    }
}
