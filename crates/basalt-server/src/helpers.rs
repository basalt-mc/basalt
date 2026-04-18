//! Shared helper types and functions for the server.

use basalt_types::{Encode, EncodedSize};

/// Converts a degree angle (f32) to a protocol byte angle (i8).
///
/// The Minecraft protocol encodes angles as bytes where 256 steps
/// represent a full 360° rotation. Rust's `as i8` saturates instead
/// of wrapping, so we cast through i32 and mask to 8 bits.
pub(crate) fn angle_to_byte(degrees: f32) -> i8 {
    ((degrees / 360.0 * 256.0) as i32 & 0xFF) as i8
}

/// A wrapper that writes raw owned bytes without any framing or encoding.
///
/// Used when we need to build a packet payload manually (e.g.,
/// PlayerInfo where the generated struct can't handle conditional
/// switch fields correctly).
pub(crate) struct RawPayload(pub Vec<u8>);

impl Encode for RawPayload {
    fn encode(&self, buf: &mut Vec<u8>) -> basalt_types::Result<()> {
        buf.extend_from_slice(&self.0);
        Ok(())
    }
}

impl EncodedSize for RawPayload {
    fn encoded_size(&self) -> usize {
        self.0.len()
    }
}

/// A wrapper that writes borrowed bytes without cloning.
///
/// Used by the net task to write cached chunk data and broadcast
/// bytes from [`SharedBroadcast`] without heap allocation.
pub(crate) struct RawSlice<'a>(pub &'a [u8]);

impl Encode for RawSlice<'_> {
    fn encode(&self, buf: &mut Vec<u8>) -> basalt_types::Result<()> {
        buf.extend_from_slice(self.0);
        Ok(())
    }
}

impl EncodedSize for RawSlice<'_> {
    fn encoded_size(&self) -> usize {
        self.0.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angle_zero() {
        assert_eq!(angle_to_byte(0.0), 0);
    }

    #[test]
    fn angle_90() {
        assert_eq!(angle_to_byte(90.0), 64);
    }

    #[test]
    fn angle_180() {
        assert_eq!(angle_to_byte(180.0), -128_i8);
    }

    #[test]
    fn angle_270() {
        assert_eq!(angle_to_byte(270.0), -64_i8);
    }

    #[test]
    fn angle_360_wraps() {
        assert_eq!(angle_to_byte(360.0), 0);
    }

    #[test]
    fn angle_negative() {
        assert_eq!(angle_to_byte(-90.0), -64_i8);
    }
}
