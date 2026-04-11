use crate::{Decode, Encode, EncodedSize, Error, Result, VarInt};

/// A variable-length bit array encoded as a VarInt-prefixed array of i64 values.
///
/// BitSet is used in the Minecraft protocol for chunk light masks, section
/// bitmasks, and other bitfield data where an arbitrary number of boolean
/// flags need to be packed efficiently. Each i64 holds 64 bits, and the
/// array grows as needed to accommodate the highest set bit.
///
/// Wire format: VarInt(number of longs) followed by that many big-endian
/// i64 values. An empty BitSet has zero longs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BitSet {
    data: Vec<i64>,
}

impl BitSet {
    /// Creates a new empty BitSet with no bits set.
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    /// Creates a BitSet from a pre-existing vector of i64 words.
    ///
    /// Each i64 holds 64 bits. The first element contains bits 0-63,
    /// the second 64-127, and so on. This is useful when constructing
    /// a BitSet from data received outside the protocol decoding path.
    pub fn from_longs(data: Vec<i64>) -> Self {
        Self { data }
    }

    /// Returns the number of bits this BitSet can currently hold without
    /// growing (i.e., the number of longs × 64).
    pub fn len(&self) -> usize {
        self.data.len() * 64
    }

    /// Returns true if the BitSet contains no longs (zero capacity).
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Returns the value of the bit at the given index.
    ///
    /// Returns `false` for indices beyond the current capacity — bits
    /// outside the allocated range are implicitly zero.
    pub fn get(&self, index: usize) -> bool {
        let word = index / 64;
        let bit = index % 64;
        if word >= self.data.len() {
            return false;
        }
        (self.data[word] >> bit) & 1 != 0
    }

    /// Sets or clears the bit at the given index.
    ///
    /// If the index is beyond the current capacity and `value` is `true`,
    /// the internal storage is automatically extended with zero-filled
    /// longs. Setting a bit to `false` beyond the current capacity is
    /// a no-op (the bit is already implicitly zero).
    pub fn set(&mut self, index: usize, value: bool) {
        let word = index / 64;
        let bit = index % 64;

        if value {
            if word >= self.data.len() {
                self.data.resize(word + 1, 0);
            }
            self.data[word] |= 1i64 << bit;
        } else if word < self.data.len() {
            self.data[word] &= !(1i64 << bit);
        }
    }
}

impl Default for BitSet {
    fn default() -> Self {
        Self::new()
    }
}

/// Encodes the BitSet as a VarInt-prefixed array of big-endian i64 values.
///
/// The VarInt indicates the number of longs in the array, followed by
/// each long encoded as 8 big-endian bytes. An empty BitSet encodes as
/// a single VarInt(0) byte.
impl Encode for BitSet {
    /// Writes VarInt(number of longs) followed by each long as 8 big-endian bytes.
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        VarInt(self.data.len() as i32).encode(buf)?;
        for &word in &self.data {
            word.encode(buf)?;
        }
        Ok(())
    }
}

/// Decodes a BitSet from a VarInt-prefixed array of big-endian i64 values.
///
/// Reads the VarInt count, then that many 8-byte big-endian i64 values.
/// Fails if the buffer doesn't contain enough bytes for the declared
/// number of longs.
impl Decode for BitSet {
    /// Reads the VarInt length, then decodes that many i64 words.
    ///
    /// Fails with `Error::BufferUnderflow` if the buffer is too short,
    /// or with `Error::InvalidData` if the declared length is negative.
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        let len = VarInt::decode(buf)?.0;
        if len < 0 {
            return Err(Error::InvalidData(format!("negative BitSet length: {len}")));
        }
        let len = len as usize;
        let mut data = Vec::with_capacity(len);
        for _ in 0..len {
            data.push(i64::decode(buf)?);
        }
        Ok(Self { data })
    }
}

/// Computes the wire size of the BitSet.
///
/// The total size is the VarInt-encoded length prefix plus 8 bytes per
/// long. This enables exact buffer pre-allocation before encoding.
impl EncodedSize for BitSet {
    /// Returns VarInt prefix size + (number of longs × 8).
    fn encoded_size(&self) -> usize {
        VarInt(self.data.len() as i32).encoded_size() + self.data.len() * 8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(bs: &BitSet) {
        let mut buf = Vec::with_capacity(bs.encoded_size());
        bs.encode(&mut buf).unwrap();
        assert_eq!(buf.len(), bs.encoded_size());

        let mut cursor = buf.as_slice();
        let decoded = BitSet::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, *bs);
    }

    // -- Construction --

    #[test]
    fn new_is_empty() {
        let bs = BitSet::new();
        assert!(bs.is_empty());
        assert_eq!(bs.len(), 0);
    }

    #[test]
    fn default_is_empty() {
        let bs = BitSet::default();
        assert!(bs.is_empty());
    }

    #[test]
    fn from_longs() {
        let bs = BitSet::from_longs(vec![0xFF, 0x00]);
        assert_eq!(bs.len(), 128);
        assert!(!bs.is_empty());
    }

    // -- Get/Set --

    #[test]
    fn get_out_of_range() {
        let bs = BitSet::new();
        assert!(!bs.get(0));
        assert!(!bs.get(1000));
    }

    #[test]
    fn set_and_get() {
        let mut bs = BitSet::new();
        bs.set(0, true);
        assert!(bs.get(0));
        assert!(!bs.get(1));
    }

    #[test]
    fn set_high_bit() {
        let mut bs = BitSet::new();
        bs.set(200, true);
        assert!(bs.get(200));
        assert!(!bs.get(199));
        assert!(!bs.get(201));
        // Should have allocated 4 longs (200 / 64 = 3, so index 3 needs 4 longs)
        assert_eq!(bs.len(), 256);
    }

    #[test]
    fn clear_bit() {
        let mut bs = BitSet::new();
        bs.set(5, true);
        assert!(bs.get(5));
        bs.set(5, false);
        assert!(!bs.get(5));
    }

    #[test]
    fn clear_out_of_range_is_noop() {
        let mut bs = BitSet::new();
        bs.set(1000, false);
        assert!(bs.is_empty());
    }

    #[test]
    fn word_boundary() {
        let mut bs = BitSet::new();
        bs.set(63, true);
        bs.set(64, true);
        assert!(bs.get(63));
        assert!(bs.get(64));
        assert!(!bs.get(62));
        assert!(!bs.get(65));
    }

    // -- Encode/Decode --

    #[test]
    fn roundtrip_empty() {
        roundtrip(&BitSet::new());
    }

    #[test]
    fn roundtrip_single_word() {
        let mut bs = BitSet::new();
        bs.set(0, true);
        bs.set(7, true);
        bs.set(63, true);
        roundtrip(&bs);
    }

    #[test]
    fn roundtrip_multiple_words() {
        let mut bs = BitSet::new();
        bs.set(0, true);
        bs.set(64, true);
        bs.set(128, true);
        roundtrip(&bs);
    }

    #[test]
    fn roundtrip_from_longs() {
        let bs = BitSet::from_longs(vec![0x0102030405060708, -1]);
        roundtrip(&bs);
    }

    #[test]
    fn empty_encodes_as_varint_zero() {
        let bs = BitSet::new();
        let mut buf = Vec::new();
        bs.encode(&mut buf).unwrap();
        assert_eq!(buf, [0x00]);
    }

    #[test]
    fn encoded_size_empty() {
        assert_eq!(BitSet::new().encoded_size(), 1);
    }

    #[test]
    fn encoded_size_one_word() {
        let bs = BitSet::from_longs(vec![1]);
        // VarInt(1) = 1 byte + 1 long = 8 bytes
        assert_eq!(bs.encoded_size(), 9);
    }

    #[test]
    fn negative_length_decode() {
        let mut buf = Vec::new();
        VarInt(-1).encode(&mut buf).unwrap();
        let mut cursor = buf.as_slice();
        assert!(matches!(
            BitSet::decode(&mut cursor),
            Err(Error::InvalidData(_))
        ));
    }

    #[test]
    fn truncated_buffer() {
        let mut buf = Vec::new();
        VarInt(2).encode(&mut buf).unwrap();
        // Only provide one long instead of two
        buf.extend_from_slice(&[0u8; 8]);
        let mut cursor = buf.as_slice();
        assert!(matches!(
            BitSet::decode(&mut cursor),
            Err(Error::BufferUnderflow { .. })
        ));
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn bitset_roundtrip(data in proptest::collection::vec(any::<i64>(), 0..10)) {
                let bs = BitSet::from_longs(data);
                roundtrip(&bs);
            }
        }
    }
}
