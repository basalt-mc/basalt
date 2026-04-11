use crate::{Decode, Encode, EncodedSize, Error, Result, VarInt};

/// Maximum byte length for a Minecraft protocol string.
const MAX_STRING_BYTES: usize = 32767;

/// Encodes a Rust `String` as a Minecraft protocol string.
///
/// Minecraft protocol strings are UTF-8 byte sequences prefixed by a VarInt
/// indicating the byte length (not character count). They are used for player
/// names, chat messages, identifiers, server addresses, and many other text
/// fields. The maximum allowed length is 32767 bytes.
impl Encode for String {
    /// Writes a VarInt length prefix followed by the UTF-8 bytes.
    ///
    /// Fails with `Error::StringTooLong` if the string exceeds 32767 bytes.
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        let bytes = self.as_bytes();
        if bytes.len() > MAX_STRING_BYTES {
            return Err(Error::StringTooLong {
                len: bytes.len(),
                max: MAX_STRING_BYTES,
            });
        }
        VarInt(bytes.len() as i32).encode(buf)?;
        buf.extend_from_slice(bytes);
        Ok(())
    }
}

/// Decodes a Minecraft protocol string into a Rust `String`.
///
/// Reads a VarInt byte length, validates it against the 32767-byte limit,
/// then reads that many bytes and validates them as UTF-8. Multi-byte
/// UTF-8 characters (accented letters, emoji, CJK) are handled correctly
/// since the length prefix counts bytes, not characters.
impl Decode for String {
    /// Reads the VarInt length prefix, then the UTF-8 payload.
    ///
    /// Fails with `Error::StringTooLong` if the declared length exceeds
    /// 32767 bytes, `Error::BufferUnderflow` if the buffer is shorter than
    /// the declared length, or `Error::InvalidUtf8` if the bytes are not
    /// valid UTF-8.
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        let len = VarInt::decode(buf)?.0 as usize;
        if len > MAX_STRING_BYTES {
            return Err(Error::StringTooLong {
                len,
                max: MAX_STRING_BYTES,
            });
        }
        if buf.len() < len {
            return Err(Error::BufferUnderflow {
                needed: len,
                available: buf.len(),
            });
        }
        let (bytes, rest) = buf.split_at(len);
        let value = String::from_utf8(bytes.to_vec())?;
        *buf = rest;
        Ok(value)
    }
}

/// Computes the wire size of a Minecraft protocol string.
///
/// The total size is the VarInt-encoded length prefix plus the UTF-8 byte
/// count. This enables exact buffer pre-allocation before encoding.
impl EncodedSize for String {
    /// Returns the VarInt prefix size plus the string's byte length.
    fn encoded_size(&self) -> usize {
        let len = self.len();
        VarInt(len as i32).encoded_size() + len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(s: &str) {
        let original = s.to_string();
        let mut buf = Vec::with_capacity(original.encoded_size());
        original.encode(&mut buf).unwrap();
        assert_eq!(buf.len(), original.encoded_size());

        let mut cursor = buf.as_slice();
        let decoded = String::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, original);
    }

    #[test]
    fn empty_string() {
        roundtrip("");
    }

    #[test]
    fn short_string() {
        roundtrip("hello");
    }

    #[test]
    fn unicode_string() {
        roundtrip("héllo wörld 🌍");
    }

    #[test]
    fn max_length_string() {
        let s = "a".repeat(MAX_STRING_BYTES);
        roundtrip(&s);
    }

    #[test]
    fn too_long_encode() {
        let s = "a".repeat(MAX_STRING_BYTES + 1);
        let mut buf = Vec::new();
        assert!(matches!(
            s.encode(&mut buf),
            Err(Error::StringTooLong { .. })
        ));
    }

    #[test]
    fn too_long_decode() {
        let mut buf = Vec::new();
        VarInt(MAX_STRING_BYTES as i32 + 1)
            .encode(&mut buf)
            .unwrap();
        buf.extend_from_slice(&vec![0u8; MAX_STRING_BYTES + 1]);

        let mut cursor = buf.as_slice();
        assert!(matches!(
            String::decode(&mut cursor),
            Err(Error::StringTooLong { .. })
        ));
    }

    #[test]
    fn truncated_buffer() {
        let mut buf = Vec::new();
        VarInt(10).encode(&mut buf).unwrap();
        buf.extend_from_slice(b"short");

        let mut cursor = buf.as_slice();
        assert!(matches!(
            String::decode(&mut cursor),
            Err(Error::BufferUnderflow { .. })
        ));
    }

    #[test]
    fn invalid_utf8() {
        let mut buf = Vec::new();
        VarInt(2).encode(&mut buf).unwrap();
        buf.extend_from_slice(&[0xFF, 0xFE]);

        let mut cursor = buf.as_slice();
        assert!(matches!(
            String::decode(&mut cursor),
            Err(Error::InvalidUtf8(_))
        ));
    }

    #[test]
    fn encoded_size_accounts_for_varint_prefix() {
        assert_eq!("".to_string().encoded_size(), 1);
        assert_eq!("hi".to_string().encoded_size(), 3);
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn string_roundtrip(s in ".{0,1000}") {
                roundtrip(&s);
            }
        }
    }
}
