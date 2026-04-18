//! Player identity for event dispatch contexts.

use basalt_types::Uuid;

use crate::components::Rotation;

/// Identity and state of the player who triggered an action.
///
/// Constructed by the server when creating a dispatch context.
/// Plugin handlers access this via `ctx.player()`.
#[derive(Debug, Clone)]
pub struct PlayerInfo {
    /// Player UUID (from Mojang or offline-mode).
    pub uuid: Uuid,
    /// Protocol entity ID.
    pub entity_id: i32,
    /// Player display name.
    pub username: String,
    /// Current facing direction.
    pub rotation: Rotation,
}
