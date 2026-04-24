//! Item and container components.

use super::Component;

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

/// Tracks that a player currently has a non-inventory window open.
///
/// Present on the player entity from the moment an OpenWindow packet
/// is sent until a CloseWindow packet is received (or the player
/// disconnects). Removed on close.
#[derive(Debug, Clone)]
pub struct OpenContainer {
    /// Protocol window ID (1-127) assigned when opening.
    pub window_id: u8,
    /// The kind of inventory that was opened.
    pub inventory_type: crate::container::InventoryType,
    /// How the container is backed in the world.
    pub backing: crate::container::ContainerBacking,
}
impl Component for OpenContainer {}

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
