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
/// item count, item ID, then component data. If no components are present,
/// explicit zero counts are written (VarInt(0) + VarInt(0)).
impl Encode for Slot {
    /// Writes the slot to the buffer.
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        VarInt(self.item_count).encode(buf)?;
        if self.item_count > 0 {
            if let Some(item_id) = self.item_id {
                VarInt(item_id).encode(buf)?;
            }
            if self.component_data.is_empty() {
                // No components: write explicit zero counts
                VarInt(0).encode(buf)?; // components to add
                VarInt(0).encode(buf)?; // components to remove
            } else {
                buf.extend_from_slice(&self.component_data);
            }
        }
        Ok(())
    }
}

/// Decodes a Slot from the Minecraft protocol format.
///
/// Reads the item count. If zero, returns an empty slot. Otherwise reads
/// the item ID and component counts. Items with zero components decode
/// cleanly, allowing correct `Vec<Slot>` support. Items with components
/// store the raw component data as opaque bytes until a full component
/// parser is implemented.
impl Decode for Slot {
    /// Reads a slot from the buffer.
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        let item_count = VarInt::decode(buf)?.0;
        if item_count <= 0 {
            return Ok(Self::empty());
        }

        let item_id = VarInt::decode(buf)?.0;

        // Read component counts to properly advance the cursor
        let num_add = VarInt::decode(buf)?.0;
        let num_remove = VarInt::decode(buf)?.0;

        if num_add == 0 && num_remove == 0 {
            // No components — cursor correctly positioned for next slot
            return Ok(Self {
                item_count,
                item_id: Some(item_id),
                component_data: Vec::new(),
            });
        }

        // Components present — re-encode counts + data as opaque for roundtrip
        let mut component_data = Vec::new();
        VarInt(num_add).encode(&mut component_data)?;
        VarInt(num_remove).encode(&mut component_data)?;

        // Components-to-remove are single VarInt type IDs
        for _ in 0..num_remove {
            let id = VarInt::decode(buf)?;
            id.encode(&mut component_data)?;
        }

        if num_add > 0 {
            // Components-to-add are variable-length and type-dependent.
            // Without a full component parser, consume remaining bytes.
            // TODO: implement component parser for full multi-slot support
            component_data.extend_from_slice(buf);
            *buf = &buf[buf.len()..];
        }

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
                + if self.component_data.is_empty() {
                    2 // VarInt(0) + VarInt(0) for zero component counts
                } else {
                    self.component_data.len()
                }
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
        // VarInt(1) = 1 byte for count + VarInt(1) = 1 byte for id
        // + VarInt(0) + VarInt(0) = 2 bytes for zero component counts = 4
        assert_eq!(slot.encoded_size(), 4);
    }

    #[test]
    fn consecutive_slot_roundtrip() {
        // Two simple slots encoded back-to-back must decode independently
        let slot1 = Slot::new(1, 10);
        let slot2 = Slot::new(2, 20);
        let mut buf = Vec::new();
        slot1.encode(&mut buf).unwrap();
        slot2.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let d1 = Slot::decode(&mut cursor).unwrap();
        let d2 = Slot::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(d1.item_count, 10);
        assert_eq!(d1.item_id, Some(1));
        assert_eq!(d2.item_count, 20);
        assert_eq!(d2.item_id, Some(2));
    }

    #[test]
    fn slot_with_remove_components_roundtrip() {
        // Build a slot with components-to-remove (0 to add, 2 to remove)
        let mut component_data = Vec::new();
        VarInt(0).encode(&mut component_data).unwrap(); // 0 to add
        VarInt(2).encode(&mut component_data).unwrap(); // 2 to remove
        VarInt(5).encode(&mut component_data).unwrap(); // remove type 5
        VarInt(10).encode(&mut component_data).unwrap(); // remove type 10

        let slot = Slot {
            item_count: 1,
            item_id: Some(42),
            component_data,
        };
        let mut buf = Vec::with_capacity(slot.encoded_size());
        slot.encode(&mut buf).unwrap();
        assert_eq!(buf.len(), slot.encoded_size());

        let mut cursor = buf.as_slice();
        let decoded = Slot::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded.item_count, 1);
        assert_eq!(decoded.item_id, Some(42));
        assert_eq!(decoded.component_data, slot.component_data);
    }

    #[test]
    fn slot_with_add_components_consumes_remaining() {
        // Build a slot with components-to-add (opaque data after counts)
        let mut component_data = Vec::new();
        VarInt(1).encode(&mut component_data).unwrap(); // 1 to add
        VarInt(0).encode(&mut component_data).unwrap(); // 0 to remove
        component_data.extend_from_slice(&[0xAA, 0xBB, 0xCC]); // opaque

        let slot = Slot {
            item_count: 1,
            item_id: Some(7),
            component_data,
        };
        let mut buf = Vec::with_capacity(slot.encoded_size());
        slot.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let decoded = Slot::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded.item_count, 1);
        assert_eq!(decoded.item_id, Some(7));
        assert_eq!(decoded.component_data, slot.component_data);
    }

    #[test]
    fn empty_and_nonempty_slots_interleaved() {
        let slots = vec![
            Slot::empty(),
            Slot::new(1, 5),
            Slot::empty(),
            Slot::new(2, 3),
        ];
        let mut buf = Vec::new();
        for s in &slots {
            s.encode(&mut buf).unwrap();
        }

        let mut cursor = buf.as_slice();
        for expected in &slots {
            let decoded = Slot::decode(&mut cursor).unwrap();
            assert_eq!(decoded.item_count, expected.item_count);
            assert_eq!(decoded.item_id, expected.item_id);
        }
        assert!(cursor.is_empty());
    }
}
