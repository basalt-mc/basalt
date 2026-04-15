use crate::{Decode, Encode, EncodedSize, Error, Result, VarInt};

/// Encodes a byte vector as a Minecraft protocol byte array.
///
/// Minecraft byte arrays are raw byte sequences prefixed by a VarInt
/// indicating the length. They are used for plugin channel data, chunk
/// sections, encryption payloads, and other binary blobs in the protocol.
/// Unlike strings, byte arrays have no encoding or length constraints
/// beyond what the packet format imposes.
impl Encode for Vec<u8> {
    /// Writes a VarInt length prefix followed by the raw bytes.
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        VarInt(self.len() as i32).encode(buf)?;
        buf.extend_from_slice(self);
        Ok(())
    }
}

/// Decodes a Minecraft protocol byte array into a `Vec<u8>`.
///
/// Reads a VarInt byte length, then reads exactly that many bytes from
/// the buffer. No validation is performed on the byte content — the
/// caller is responsible for interpreting the data.
impl Decode for Vec<u8> {
    /// Reads the VarInt length prefix, then copies the raw payload.
    ///
    /// Fails with `Error::BufferUnderflow` if the buffer is shorter
    /// than the declared length.
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        let raw_len = VarInt::decode(buf)?.0;
        if raw_len < 0 {
            return Err(Error::InvalidData(format!(
                "negative byte array length: {raw_len}"
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
        Ok(value)
    }
}

/// Computes the wire size of a Minecraft protocol byte array.
///
/// The total size is the VarInt-encoded length prefix plus the byte count.
/// This enables exact buffer pre-allocation before encoding.
impl EncodedSize for Vec<u8> {
    /// Returns the VarInt prefix size plus the array's byte length.
    fn encoded_size(&self) -> usize {
        VarInt(self.len() as i32).encoded_size() + self.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(data: &[u8]) {
        let original = data.to_vec();
        let mut buf = Vec::with_capacity(original.encoded_size());
        original.encode(&mut buf).unwrap();
        assert_eq!(buf.len(), original.encoded_size());

        let mut cursor = buf.as_slice();
        let decoded = Vec::<u8>::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, original);
    }

    #[test]
    fn empty() {
        roundtrip(&[]);
    }

    #[test]
    fn small() {
        roundtrip(&[0x01, 0x02, 0x03]);
    }

    #[test]
    fn large() {
        roundtrip(&vec![0xAB; 1024]);
    }

    #[test]
    fn truncated_buffer() {
        let mut buf = Vec::new();
        VarInt(10).encode(&mut buf).unwrap();
        buf.extend_from_slice(&[0x01; 5]);

        let mut cursor = buf.as_slice();
        assert!(matches!(
            Vec::<u8>::decode(&mut cursor),
            Err(Error::BufferUnderflow { .. })
        ));
    }

    #[test]
    fn encoded_size_accounts_for_varint_prefix() {
        assert_eq!(Vec::<u8>::new().encoded_size(), 1);
        assert_eq!(vec![0u8; 3].encoded_size(), 4);
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn byte_array_roundtrip(data in proptest::collection::vec(any::<u8>(), 0..1000)) {
                roundtrip(&data);
            }
        }
    }
}
