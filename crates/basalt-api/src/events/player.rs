//! Player lifecycle and movement events.

use basalt_core::{ChunkPosition, Position, Rotation};

/// A player moved or changed look direction.
///
/// Fired after the position is updated. Not cancellable — the server
/// is not authoritative for position in vanilla Minecraft.
/// The moving player is available via `ctx.player()`.
#[derive(Debug, Clone)]
pub struct PlayerMovedEvent {
    /// New absolute position.
    pub position: Position,
    /// New facing direction.
    pub rotation: Rotation,
    /// Whether the player is on the ground.
    pub on_ground: bool,
    /// Chunk position before the movement (for boundary detection).
    pub old_chunk: ChunkPosition,
}
crate::game_event!(PlayerMovedEvent);

/// A new player has joined the server.
///
/// Not cancellable — the player is already connected. The joining
/// player's identity is available via `ctx.player()`.
#[derive(Debug, Clone)]
pub struct PlayerJoinedEvent;
crate::game_event!(PlayerJoinedEvent);

/// A player has disconnected from the server.
///
/// Not cancellable — the connection is already closed. The leaving
/// player's identity is available via `ctx.player()`.
#[derive(Debug, Clone)]
pub struct PlayerLeftEvent;
crate::game_event!(PlayerLeftEvent);

#[cfg(test)]
mod tests {
    use crate::events::Event;

    use super::*;

    #[test]
    fn player_moved_not_cancellable() {
        let mut event = PlayerMovedEvent {
            position: Position {
                x: 0.0,
                y: 64.0,
                z: 0.0,
            },
            rotation: Rotation {
                yaw: 0.0,
                pitch: 0.0,
            },
            on_ground: true,
            old_chunk: ChunkPosition { x: 0, z: 0 },
        };
        event.cancel(); // no-op
        assert!(!event.is_cancelled());
    }

    #[test]
    fn event_routing() {
        use crate::events::{BusKind, EventRouting};
        assert_eq!(PlayerMovedEvent::BUS, BusKind::Game);
        assert_eq!(PlayerJoinedEvent::BUS, BusKind::Game);
        assert_eq!(PlayerLeftEvent::BUS, BusKind::Game);
    }
}
