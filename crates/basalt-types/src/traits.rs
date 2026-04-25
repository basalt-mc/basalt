use crate::Result;

/// Serialize a value into a byte buffer.
pub trait Encode {
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()>;
}

/// Deserialize a value from a byte slice, advancing the cursor.
pub trait Decode: Sized {
    fn decode(buf: &mut &[u8]) -> Result<Self>;
}

/// Predict the exact serialized byte count for pre-allocation.
pub trait EncodedSize {
    fn encoded_size(&self) -> usize;
}

// Blanket `Encode` / `Decode` / `EncodedSize` impls for `Box<T>`.
//
// The codegen wraps recursive struct/variant fields in `Box` to break
// the infinite-size cycle (e.g. `SlotDisplay::SmithingTrim { base:
// Box<SlotDisplay> }`). The wire format is identical to the inner
// `T`'s — `Box` is a Rust-side concern only — so we forward to `T`
// through a deref.

impl<T: Encode + ?Sized> Encode for Box<T> {
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        (**self).encode(buf)
    }
}

impl<T: Decode> Decode for Box<T> {
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        T::decode(buf).map(Box::new)
    }
}

impl<T: EncodedSize + ?Sized> EncodedSize for Box<T> {
    fn encoded_size(&self) -> usize {
        (**self).encoded_size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    /// Dummy type to verify the traits compile and work together.
    struct DummyByte(u8);

    impl Encode for DummyByte {
        fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
            buf.push(self.0);
            Ok(())
        }
    }

    impl Decode for DummyByte {
        fn decode(buf: &mut &[u8]) -> Result<Self> {
            if buf.is_empty() {
                return Err(Error::BufferUnderflow {
                    needed: 1,
                    available: 0,
                });
            }
            let value = buf[0];
            *buf = &buf[1..];
            Ok(DummyByte(value))
        }
    }

    impl EncodedSize for DummyByte {
        fn encoded_size(&self) -> usize {
            1
        }
    }

    #[test]
    fn roundtrip() {
        let original = DummyByte(42);
        let mut buf = Vec::with_capacity(original.encoded_size());
        original.encode(&mut buf).unwrap();
        assert_eq!(buf.len(), original.encoded_size());

        let mut cursor = buf.as_slice();
        let decoded = DummyByte::decode(&mut cursor).unwrap();
        assert_eq!(decoded.0, 42);
        assert!(cursor.is_empty());
    }

    #[test]
    fn decode_empty_buffer() {
        let mut cursor: &[u8] = &[];
        let result = DummyByte::decode(&mut cursor);
        assert!(result.is_err());
    }
}
