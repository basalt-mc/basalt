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

/// Tracks an open container window for a player.
///
/// Present on player entities while they have a container (chest, etc.)
/// open. Used to route WindowClick packets and broadcast slot changes
/// to all viewers of the same container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenContainer {
    /// Protocol window ID (1-127, cycling).
    pub window_id: u8,
    /// Absolute block position of the container.
    pub position: (i32, i32, i32),
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
