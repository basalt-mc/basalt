//! Entity identity and state components.

use crate::components::Component;

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

/// Marker component for sneaking state.
///
/// Added when a player starts sneaking, removed when they stop.
/// Used by the block interaction system to determine whether
/// right-click should interact with the clicked block or place
/// a block instead.
pub struct Sneaking;
impl Component for Sneaking {}
