//! Player identity for event dispatch contexts.

use basalt_types::Uuid;

use crate::components::{Position, Rotation};

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
    /// Current world position. Read at context-construction time, so
    /// it reflects the player's location when the event fired —
    /// stale by the next tick.
    pub position: Position,
}

impl PlayerInfo {
    /// Returns a sentinel [`PlayerInfo`] for system-level dispatches.
    ///
    /// Used during plugin loading when the dispatch context exists but
    /// no player is involved (e.g. recipe registry lifecycle events).
    /// The `entity_id` is `-1`, the username is `"<system>"`, and all
    /// position / rotation fields are zero. Plugin handlers receiving
    /// these dispatches must not rely on `ctx.player()` data — the
    /// event payload carries everything they need.
    pub fn stub() -> Self {
        Self {
            uuid: Uuid::default(),
            entity_id: -1,
            username: String::from("<system>"),
            rotation: Rotation {
                yaw: 0.0,
                pitch: 0.0,
            },
            position: Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_is_zeroed() {
        let p = PlayerInfo::stub();
        assert_eq!(p.uuid, Uuid::default());
        assert_eq!(p.entity_id, -1);
        assert_eq!(p.username, "<system>");
        assert_eq!(p.position.x, 0.0);
        assert_eq!(p.rotation.yaw, 0.0);
    }
}
