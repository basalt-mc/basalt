use crate::{Decode, Encode, EncodedSize, Result, VarInt};

/// A Minecraft item stack, used in inventories, entity equipment, and trade offers.
///
/// The Slot type represents an item stack in the Minecraft protocol. It encodes
/// the item count, item ID, and optional component data. An empty slot has
/// `item_count = 0` and no other data. A non-empty slot includes the item ID
/// and an optional list of item components (like damage, enchantments, custom name).
///
/// Wire format:
/// - VarInt `item_count` (0 = empty slot, > 0 = slot with item)
/// - If `item_count > 0`:
///   - VarInt `item_id`
///   - VarInt `num_components_to_add`
///   - VarInt `num_components_to_remove`
///   - Component data (opaque bytes for now)
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Slot {
    /// The number of items in this stack. 0 means the slot is empty.
    pub item_count: i32,
    /// The registry ID of the item, if the slot is not empty.
    pub item_id: Option<i32>,
    /// Raw component data bytes. Full component parsing is deferred
    /// to a future implementation — for now, the components are stored
    /// as opaque bytes to allow roundtrip encoding.
    pub component_data: Vec<u8>,
}

impl Slot {
    /// Creates an empty slot (no item).
    pub fn empty() -> Self {
        Self {
            item_count: 0,
            item_id: None,
            component_data: Vec::new(),
        }
    }

    /// Creates a simple slot with an item ID and count, no components.
    pub fn new(item_id: i32, count: i32) -> Self {
        Self {
            item_count: count,
            item_id: Some(item_id),
            component_data: Vec::new(),
        }
    }

    /// Returns true if the slot is empty (no item).
    pub fn is_empty(&self) -> bool {
        self.item_count == 0
    }
}

/// Encodes a Slot in the Minecraft protocol format.
///
/// Empty slots encode as a single VarInt(0). Non-empty slots encode the
/// item count, item ID, then component counts (both set to 0 for now)
/// since we don't parse individual components.
impl Encode for Slot {
    /// Writes the slot to the buffer.
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        VarInt(self.item_count).encode(buf)?;
        if self.item_count > 0 {
            if let Some(item_id) = self.item_id {
                VarInt(item_id).encode(buf)?;
            }
            buf.extend_from_slice(&self.component_data);
        }
        Ok(())
    }
}

/// Decodes a Slot from the Minecraft protocol format.
///
/// Reads the item count. If zero, returns an empty slot. Otherwise reads
/// the item ID and stores remaining component data as raw bytes.
impl Decode for Slot {
    /// Reads a slot from the buffer.
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        let item_count = VarInt::decode(buf)?.0;
        if item_count == 0 {
            return Ok(Self::empty());
        }

        let item_id = VarInt::decode(buf)?.0;

        // Read component counts and data as opaque bytes
        // Components are complex (switch on type) — we store them raw
        let component_data = buf.to_vec();
        *buf = &buf[buf.len()..];

        Ok(Self {
            item_count,
            item_id: Some(item_id),
            component_data,
        })
    }
}

/// Computes the wire size of a Slot.
impl EncodedSize for Slot {
    fn encoded_size(&self) -> usize {
        let count_size = VarInt(self.item_count).encoded_size();
        if self.item_count == 0 {
            count_size
        } else {
            count_size
                + self.item_id.map_or(0, |id| VarInt(id).encoded_size())
                + self.component_data.len()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_slot_roundtrip() {
        let slot = Slot::empty();
        let mut buf = Vec::new();
        slot.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let decoded = Slot::decode(&mut cursor).unwrap();
        assert!(decoded.is_empty());
        assert_eq!(decoded.item_count, 0);
    }

    #[test]
    fn empty_slot_encodes_as_zero() {
        let slot = Slot::empty();
        let mut buf = Vec::new();
        slot.encode(&mut buf).unwrap();
        assert_eq!(buf, [0x00]);
    }

    #[test]
    fn simple_slot() {
        let slot = Slot::new(1, 64);
        assert!(!slot.is_empty());
        assert_eq!(slot.item_count, 64);
        assert_eq!(slot.item_id, Some(1));
    }

    #[test]
    fn encoded_size_empty() {
        let slot = Slot::empty();
        assert_eq!(slot.encoded_size(), 1);
    }

    #[test]
    fn default_is_empty() {
        let slot = Slot::default();
        assert!(slot.is_empty());
    }

    #[test]
    fn non_empty_slot_encode_decode() {
        let slot = Slot::new(42, 10);
        let mut buf = Vec::with_capacity(slot.encoded_size());
        slot.encode(&mut buf).unwrap();
        assert_eq!(buf.len(), slot.encoded_size());

        // Decode reads item_count + item_id + consumes rest as components
        let mut cursor = buf.as_slice();
        let decoded = Slot::decode(&mut cursor).unwrap();
        assert_eq!(decoded.item_count, 10);
        assert_eq!(decoded.item_id, Some(42));
    }

    #[test]
    fn encoded_size_non_empty() {
        let slot = Slot::new(1, 1);
        // VarInt(1) = 1 byte for count + VarInt(1) = 1 byte for id = 2
        assert_eq!(slot.encoded_size(), 2);
    }
}
