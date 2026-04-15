//! Opaque byte buffer for protocol types that can't be parsed.
//!
//! `OpaqueBytes` wraps a `Vec<u8>` to distinguish intentionally opaque
//! protocol data (native types, switch fallbacks) from other byte
//! vectors that happen to use `Vec<u8>`. It encodes/decodes as a
//! length-prefixed byte array, same as `Vec<u8>`.

use crate::{Decode, Encode, EncodedSize, Error, Result, VarInt};

/// A byte buffer for protocol data whose internal structure is not
/// parsed by the codec.
///
/// Used for native protocol types (`entityMetadata`, `registryEntryHolder`,
/// etc.) and switch field fallbacks where the wire format is known
/// but too complex to represent in the type system.
///
/// Unlike raw `Vec<u8>`, `OpaqueBytes` makes the intent explicit:
/// "this data is intentionally unparsed".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OpaqueBytes(pub Vec<u8>);

impl OpaqueBytes {
    /// Creates an empty opaque buffer.
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Creates an opaque buffer from raw bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Returns a reference to the inner bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Returns the number of bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Encode for OpaqueBytes {
    /// Encodes as a VarInt length prefix followed by the raw bytes.
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        VarInt(self.0.len() as i32).encode(buf)?;
        buf.extend_from_slice(&self.0);
        Ok(())
    }
}

impl Decode for OpaqueBytes {
    /// Decodes a VarInt length prefix then reads that many bytes.
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        let raw_len = VarInt::decode(buf)?.0;
        if raw_len < 0 {
            return Err(Error::InvalidData(format!(
                "negative opaque length: {raw_len}"
            )));
        }
        let len = raw_len as usize;
        if buf.len() < len {
            return Err(Error::BufferUnderflow {
                needed: len,
                available: buf.len(),
            });
        }
        let (bytes, rest) = buf.split_at(len);
        let value = bytes.to_vec();
        *buf = rest;
        Ok(Self(value))
    }
}

impl EncodedSize for OpaqueBytes {
    fn encoded_size(&self) -> usize {
        VarInt(self.0.len() as i32).encoded_size() + self.0.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_roundtrip() {
        let original = OpaqueBytes::new();
        let mut buf = Vec::new();
        original.encode(&mut buf).unwrap();
        assert_eq!(buf.len(), 1); // VarInt(0)

        let mut cursor = buf.as_slice();
        let decoded = OpaqueBytes::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, original);
    }

    #[test]
    fn data_roundtrip() {
        let original = OpaqueBytes::from_bytes(vec![1, 2, 3, 4, 5]);
        let mut buf = Vec::with_capacity(original.encoded_size());
        original.encode(&mut buf).unwrap();
        assert_eq!(buf.len(), original.encoded_size());

        let mut cursor = buf.as_slice();
        let decoded = OpaqueBytes::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, original);
    }

    #[test]
    fn accessors() {
        let opaque = OpaqueBytes::from_bytes(vec![0xAB, 0xCD]);
        assert_eq!(opaque.len(), 2);
        assert!(!opaque.is_empty());
        assert_eq!(opaque.as_bytes(), &[0xAB, 0xCD]);
    }
}
