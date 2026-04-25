//! Player inventory component with protocol slot remapping.

use crate::components::Component;

/// Player inventory — 36 slots (27 main + 9 hotbar).
///
/// Slot layout matches Minecraft's raw player inventory:
/// - Slots 0–8: hotbar
/// - Slots 9–35: main inventory (3 rows of 9)
///
/// This matches the `SetPlayerInventory` packet (1.21.4) directly —
/// no slot conversion needed when syncing individual slots.
///
/// For the player inventory *window* (SetContainerContent), slots remap:
/// window 9-35 = main (our 9-35, same), window 36-44 = hotbar (our 0-8).
#[derive(Debug, Clone)]
pub struct Inventory {
    /// Currently selected hotbar index (0-8).
    pub held_slot: u8,
    /// All 36 inventory slots (0-8 hotbar, 9-35 main).
    pub slots: [basalt_types::Slot; 36],
    /// Item currently held on the mouse cursor (in an open window).
    pub cursor: basalt_types::Slot,
}

impl Inventory {
    /// First hotbar slot index within `slots`.
    pub const HOTBAR_START: usize = 0;
    /// First main inventory slot index within `slots`.
    pub const MAIN_START: usize = 9;

    /// Creates an empty inventory with slot 0 selected.
    pub fn empty() -> Self {
        Self {
            held_slot: 0,
            slots: std::array::from_fn(|_| basalt_types::Slot::empty()),
            cursor: basalt_types::Slot::empty(),
        }
    }

    /// Returns the currently held item (from the hotbar).
    pub fn held_item(&self) -> &basalt_types::Slot {
        &self.slots[self.held_slot as usize]
    }

    /// Returns a reference to the hotbar (9 slots).
    pub fn hotbar(&self) -> &[basalt_types::Slot] {
        &self.slots[..9]
    }

    /// Returns a mutable reference to the hotbar (9 slots).
    pub fn hotbar_mut(&mut self) -> &mut [basalt_types::Slot] {
        &mut self.slots[..9]
    }

    /// Converts a protocol window slot to an internal slot index.
    ///
    /// Window layout: 9-35 = main, 36-44 = hotbar.
    /// Internal layout: 0-8 = hotbar, 9-35 = main.
    pub fn window_to_index(window_slot: i16) -> Option<usize> {
        match window_slot {
            9..=35 => Some(window_slot as usize), // main: same numbering
            36..=44 => Some((window_slot - 36) as usize), // hotbar: window 36-44 → 0-8
            _ => None,
        }
    }

    /// Converts an internal slot index to a protocol window slot.
    pub fn index_to_window(index: usize) -> Option<i16> {
        match index {
            0..=8 => Some(index as i16 + 36), // hotbar → window 36-44
            9..=35 => Some(index as i16),     // main: same numbering
            _ => None,
        }
    }

    /// Tries to insert an item into the inventory.
    ///
    /// Searches hotbar first (for convenience), then main inventory.
    /// Tries matching stacks (count < 64) first, then empty slots.
    /// Returns `Some(internal_index)` if inserted, `None` if full.
    pub fn try_insert(&mut self, item_id: i32, count: i32) -> Option<usize> {
        // Hotbar first, then main — matching stacks
        let search_order = (0..9).chain(Self::MAIN_START..36);
        for i in search_order {
            let slot = &mut self.slots[i];
            if slot.item_id == Some(item_id) && slot.item_count < 64 {
                let space = 64 - slot.item_count;
                let to_add = count.min(space);
                slot.item_count += to_add;
                if to_add == count {
                    return Some(i);
                }
            }
        }
        // Hotbar first, then main — empty slots
        let search_order = (0..9).chain(Self::MAIN_START..36);
        for i in search_order {
            if self.slots[i].is_empty() {
                self.slots[i] = basalt_types::Slot::new(item_id, count);
                return Some(i);
            }
        }
        None
    }

    /// Builds the 46-slot protocol representation for SetContainerContent.
    ///
    /// Window slot layout for player inventory (id=0):
    /// 0 = crafting output, 1-4 = crafting grid, 5-8 = armor,
    /// 9-35 = main inventory, 36-44 = hotbar, 45 = offhand.
    pub fn to_protocol_slots(&self) -> Vec<basalt_types::Slot> {
        let mut protocol = vec![basalt_types::Slot::empty(); 46];
        // Main: internal 9-35 → window 9-35 (same)
        protocol[9..36].clone_from_slice(&self.slots[9..]);
        // Hotbar: internal 0-8 → window 36-44
        protocol[36..45].clone_from_slice(&self.slots[..9]);
        protocol
    }
}
impl Component for Inventory {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_insert_empty_hotbar() {
        let mut inv = Inventory::empty();
        let idx = inv.try_insert(1, 1);
        assert_eq!(idx, Some(Inventory::HOTBAR_START));
        assert_eq!(inv.slots[Inventory::HOTBAR_START].item_id, Some(1));
    }

    #[test]
    fn try_insert_stacks() {
        let mut inv = Inventory::empty();
        inv.try_insert(1, 32);
        let idx = inv.try_insert(1, 16);
        assert_eq!(idx, Some(Inventory::HOTBAR_START));
        assert_eq!(inv.slots[Inventory::HOTBAR_START].item_count, 48);
    }

    #[test]
    fn try_insert_full_returns_none() {
        let mut inv = Inventory::empty();
        for i in 0..36 {
            inv.slots[i] = basalt_types::Slot::new(i as i32 + 100, 64);
        }
        assert_eq!(inv.try_insert(999, 1), None);
    }

    #[test]
    fn slot_conversion() {
        assert_eq!(Inventory::window_to_index(9), Some(9));
        assert_eq!(Inventory::window_to_index(35), Some(35));
        assert_eq!(Inventory::window_to_index(36), Some(0));
        assert_eq!(Inventory::window_to_index(44), Some(8));
        assert_eq!(Inventory::window_to_index(0), None);
        assert_eq!(Inventory::window_to_index(45), None);
        assert_eq!(Inventory::index_to_window(0), Some(36));
        assert_eq!(Inventory::index_to_window(9), Some(9));
    }

    #[test]
    fn to_protocol_slots_length() {
        let inv = Inventory::empty();
        assert_eq!(inv.to_protocol_slots().len(), 46);
    }

    #[test]
    fn to_protocol_slots_maps_correctly() {
        let mut inv = Inventory::empty();
        inv.slots[0] = basalt_types::Slot::new(1, 1);
        inv.slots[9] = basalt_types::Slot::new(2, 2);
        let proto = inv.to_protocol_slots();
        assert_eq!(proto[36].item_id, Some(1));
        assert_eq!(proto[9].item_id, Some(2));
    }

    #[test]
    fn held_item_and_hotbar() {
        let mut inv = Inventory::empty();
        inv.slots[3] = basalt_types::Slot::new(5, 10);
        inv.held_slot = 3;
        assert_eq!(inv.held_item().item_id, Some(5));
        assert_eq!(inv.hotbar().len(), 9);
        assert_eq!(inv.hotbar()[3].item_count, 10);
    }

    #[test]
    fn try_insert_main_when_hotbar_full() {
        let mut inv = Inventory::empty();
        for i in 0..9 {
            inv.slots[i] = basalt_types::Slot::new(i as i32, 64);
        }
        let idx = inv.try_insert(999, 1);
        assert_eq!(idx, Some(Inventory::MAIN_START));
        assert_eq!(inv.slots[Inventory::MAIN_START].item_id, Some(999));
    }

    #[test]
    fn window_roundtrip() {
        for i in 0..36 {
            let window = Inventory::index_to_window(i).unwrap();
            let back = Inventory::window_to_index(window).unwrap();
            assert_eq!(back, i);
        }
    }
}
