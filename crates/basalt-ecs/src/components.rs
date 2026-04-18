//! Core components provided by the ECS.
//!
//! These are the basic building blocks for entity state. Plugins
//! can register additional custom components via the registrar.

use crate::ecs::Component;

/// World position of an entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
    /// Z coordinate.
    pub z: f64,
}
impl Component for Position {}

/// Facing direction of an entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rotation {
    /// Horizontal angle in degrees.
    pub yaw: f32,
    /// Vertical angle in degrees.
    pub pitch: f32,
}
impl Component for Rotation {}

/// Movement vector per tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Velocity {
    /// X movement per tick.
    pub dx: f64,
    /// Y movement per tick.
    pub dy: f64,
    /// Z movement per tick.
    pub dz: f64,
}
impl Component for Velocity {}

/// AABB hitbox dimensions.
///
/// The bounding box is axis-aligned and centered on the entity's
/// position. Used for collision detection and entity interactions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    /// Width (X and Z extent).
    pub width: f32,
    /// Height (Y extent).
    pub height: f32,
}
impl Component for BoundingBox {}

/// Minecraft entity type ID.
///
/// Maps to the registry entity type (e.g., 147 = player in 1.21.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityKind {
    /// Registry type ID.
    pub type_id: u32,
}
impl Component for EntityKind {}

/// Hit points for damageable entities.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Health {
    /// Current health.
    pub current: f32,
    /// Maximum health.
    pub max: f32,
}
impl Component for Health {}

/// Auto-despawn countdown.
///
/// Decremented each tick. When it reaches zero, the entity is
/// despawned. Used for dropped items (5 minutes = 6000 ticks),
/// arrows, experience orbs, etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lifetime {
    /// Remaining ticks before despawn.
    pub remaining_ticks: u32,
}
impl Component for Lifetime {}

/// Links an entity to a player connection.
///
/// Present on player entities to map between the ECS entity and
/// the player's network state (UUID, username, output channel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRef {
    /// Player UUID (from Mojang or offline-mode).
    pub uuid: basalt_types::Uuid,
    /// Player display name.
    pub username: String,
}
impl Component for PlayerRef {}

/// A dropped item on the ground.
///
/// Entities with this component represent items that can be picked up
/// by players. Combined with `Position`, `Velocity`, `BoundingBox`,
/// and `Lifetime` to create a full dropped item entity.
#[derive(Debug, Clone)]
pub struct DroppedItem {
    /// The item stack (ID, count, component data).
    pub slot: basalt_types::Slot,
}
impl Component for DroppedItem {}

/// Pickup delay before a dropped item can be collected.
///
/// Decremented each tick. While remaining > 0, the item cannot be
/// picked up by any player. Default: 10 ticks (0.5s) for block drops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickupDelay {
    /// Remaining ticks before the item is pickable.
    pub remaining_ticks: u32,
}
impl Component for PickupDelay {}

/// Player inventory — 36 slots (27 main + 9 hotbar).
///
/// Slot layout matches the Minecraft player inventory window:
/// - Slots 0–26: main inventory (3 rows of 9)
/// - Slots 27–35: hotbar (bottom row)
///
/// In the protocol window, these map to:
/// - Protocol slots 9–35: main inventory (our 0–26)
/// - Protocol slots 36–44: hotbar (our 27–35)
#[derive(Debug, Clone)]
pub struct Inventory {
    /// Currently selected hotbar index (0-8, relative to hotbar start).
    pub held_slot: u8,
    /// All 36 inventory slots.
    pub slots: [basalt_types::Slot; 36],
}

impl Inventory {
    /// First hotbar slot index within `slots`.
    pub const HOTBAR_START: usize = 27;

    /// Creates an empty inventory with slot 0 selected.
    pub fn empty() -> Self {
        Self {
            held_slot: 0,
            slots: std::array::from_fn(|_| basalt_types::Slot::empty()),
        }
    }

    /// Returns the currently held item (from the hotbar).
    pub fn held_item(&self) -> &basalt_types::Slot {
        &self.slots[Self::HOTBAR_START + self.held_slot as usize]
    }

    /// Returns a reference to the hotbar (9 slots).
    pub fn hotbar(&self) -> &[basalt_types::Slot] {
        &self.slots[Self::HOTBAR_START..]
    }

    /// Returns a mutable reference to the hotbar (9 slots).
    pub fn hotbar_mut(&mut self) -> &mut [basalt_types::Slot] {
        &mut self.slots[Self::HOTBAR_START..]
    }

    /// Converts a protocol slot index to an internal slot index.
    ///
    /// Protocol layout: 9-35 = main, 36-44 = hotbar.
    /// Internal layout: 0-26 = main, 27-35 = hotbar.
    /// Returns `None` for out-of-range or non-inventory slots (0-8 = crafting/armor).
    pub fn protocol_to_index(protocol_slot: i16) -> Option<usize> {
        match protocol_slot {
            9..=35 => Some((protocol_slot - 9) as usize), // main
            36..=44 => Some((protocol_slot - 36 + 27) as usize), // hotbar
            _ => None,
        }
    }

    /// Converts an internal slot index to a protocol slot index.
    pub fn index_to_protocol(index: usize) -> Option<i16> {
        match index {
            0..=26 => Some(index as i16 + 9),        // main
            27..=35 => Some(index as i16 - 27 + 36), // hotbar
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
        let search_order = (Self::HOTBAR_START..36).chain(0..Self::HOTBAR_START);
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
        let search_order = (Self::HOTBAR_START..36).chain(0..Self::HOTBAR_START);
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
    /// Protocol slot layout for player inventory window (id=0):
    /// 0 = crafting output, 1-4 = crafting grid, 5-8 = armor,
    /// 9-35 = main inventory, 36-44 = hotbar, 45 = offhand.
    pub fn to_protocol_slots(&self) -> Vec<basalt_types::Slot> {
        let mut protocol = vec![basalt_types::Slot::empty(); 46];
        // Main inventory: internal 0-26 → protocol 9-35
        protocol[9..36].clone_from_slice(&self.slots[..27]);
        // Hotbar: internal 27-35 → protocol 36-44
        protocol[36..45].clone_from_slice(&self.slots[27..]);
        protocol
    }
}
impl Component for Inventory {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Ecs;

    #[test]
    fn all_components_work_with_ecs() {
        let mut ecs = Ecs::new();
        let e = ecs.spawn();

        ecs.set(
            e,
            Position {
                x: 0.0,
                y: 64.0,
                z: 0.0,
            },
        );
        ecs.set(
            e,
            Rotation {
                yaw: 90.0,
                pitch: 0.0,
            },
        );
        ecs.set(
            e,
            Velocity {
                dx: 0.1,
                dy: -0.08,
                dz: 0.0,
            },
        );
        ecs.set(
            e,
            BoundingBox {
                width: 0.6,
                height: 1.8,
            },
        );
        ecs.set(e, EntityKind { type_id: 147 });
        ecs.set(
            e,
            Health {
                current: 20.0,
                max: 20.0,
            },
        );
        ecs.set(
            e,
            Lifetime {
                remaining_ticks: 6000,
            },
        );
        ecs.set(
            e,
            PlayerRef {
                uuid: basalt_types::Uuid::default(),
                username: "Steve".into(),
            },
        );

        ecs.set(
            e,
            PickupDelay {
                remaining_ticks: 10,
            },
        );

        assert!(ecs.has::<Position>(e));
        assert!(ecs.has::<Rotation>(e));
        assert!(ecs.has::<Velocity>(e));
        assert!(ecs.has::<BoundingBox>(e));
        assert!(ecs.has::<EntityKind>(e));
        assert!(ecs.has::<Health>(e));
        assert!(ecs.has::<Lifetime>(e));
        assert!(ecs.has::<PlayerRef>(e));
        assert!(ecs.has::<PickupDelay>(e));
    }

    #[test]
    fn inventory_try_insert_empty_hotbar() {
        let mut inv = Inventory::empty();
        let idx = inv.try_insert(1, 1);
        assert_eq!(idx, Some(Inventory::HOTBAR_START)); // first hotbar slot
        assert_eq!(inv.slots[Inventory::HOTBAR_START].item_id, Some(1));
    }

    #[test]
    fn inventory_try_insert_stacks() {
        let mut inv = Inventory::empty();
        inv.try_insert(1, 32);
        let idx = inv.try_insert(1, 16);
        assert_eq!(idx, Some(Inventory::HOTBAR_START)); // same slot
        assert_eq!(inv.slots[Inventory::HOTBAR_START].item_count, 48);
    }

    #[test]
    fn inventory_try_insert_full_returns_none() {
        let mut inv = Inventory::empty();
        for i in 0..36 {
            inv.slots[i] = basalt_types::Slot::new(i as i32 + 100, 64);
        }
        assert_eq!(inv.try_insert(999, 1), None);
    }

    #[test]
    fn inventory_protocol_slot_conversion() {
        // Main: protocol 9-35 → internal 0-26
        assert_eq!(Inventory::protocol_to_index(9), Some(0));
        assert_eq!(Inventory::protocol_to_index(35), Some(26));
        // Hotbar: protocol 36-44 → internal 27-35
        assert_eq!(Inventory::protocol_to_index(36), Some(27));
        assert_eq!(Inventory::protocol_to_index(44), Some(35));
        // Out of range
        assert_eq!(Inventory::protocol_to_index(0), None);
        assert_eq!(Inventory::protocol_to_index(45), None);
        // Roundtrip
        assert_eq!(Inventory::index_to_protocol(0), Some(9));
        assert_eq!(Inventory::index_to_protocol(27), Some(36));
    }

    #[test]
    fn inventory_to_protocol_slots_length() {
        let inv = Inventory::empty();
        assert_eq!(inv.to_protocol_slots().len(), 46);
    }

    #[test]
    fn inventory_to_protocol_slots_maps_correctly() {
        let mut inv = Inventory::empty();
        inv.slots[0] = basalt_types::Slot::new(1, 1); // main slot 0
        inv.slots[27] = basalt_types::Slot::new(2, 2); // hotbar slot 0
        let proto = inv.to_protocol_slots();
        assert_eq!(proto[9].item_id, Some(1)); // main → protocol 9
        assert_eq!(proto[36].item_id, Some(2)); // hotbar → protocol 36
    }

    #[test]
    fn inventory_held_item_and_hotbar() {
        let mut inv = Inventory::empty();
        inv.slots[Inventory::HOTBAR_START + 3] = basalt_types::Slot::new(5, 10);
        inv.held_slot = 3;
        assert_eq!(inv.held_item().item_id, Some(5));
        assert_eq!(inv.hotbar().len(), 9);
        assert_eq!(inv.hotbar()[3].item_count, 10);
    }

    #[test]
    fn inventory_try_insert_main_when_hotbar_full() {
        let mut inv = Inventory::empty();
        // Fill hotbar
        for i in 0..9 {
            inv.slots[Inventory::HOTBAR_START + i] = basalt_types::Slot::new(i as i32, 64);
        }
        // Should go to main inventory (slot 0)
        let idx = inv.try_insert(999, 1);
        assert_eq!(idx, Some(0));
        assert_eq!(inv.slots[0].item_id, Some(999));
    }

    #[test]
    fn inventory_index_to_protocol_roundtrip() {
        for i in 0..36 {
            let proto = Inventory::index_to_protocol(i).unwrap();
            let back = Inventory::protocol_to_index(proto).unwrap();
            assert_eq!(back, i);
        }
    }
}
