//! Concrete game events dispatched through the event bus.
//!
//! Each event struct carries domain data relevant to a specific game
//! action. Cancellable events have a `cancelled` field that Validate
//! handlers can set to prevent Process and Post handlers from running.
//!
//! Use the [`cancellable_event!`] and [`event!`] macros to implement
//! the [`Event`](basalt_events::Event) trait for custom event types.

use basalt_types::Uuid;

use crate::broadcast::PlayerSnapshot;

/// Implements [`Event`](basalt_events::Event) for a cancellable event struct.
///
/// The struct must have a `cancelled: bool` field. Validate handlers
/// can call `event.cancel()` to prevent Process and Post handlers
/// from running.
///
/// # Example
///
/// ```ignore
/// pub struct MyEvent {
///     pub data: String,
///     pub cancelled: bool,
/// }
/// basalt_api::cancellable_event!(MyEvent);
/// ```
#[macro_export]
macro_rules! cancellable_event {
    ($name:ident) => {
        impl basalt_events::Event for $name {
            fn is_cancelled(&self) -> bool {
                self.cancelled
            }
            fn cancel(&mut self) {
                self.cancelled = true;
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
        }
    };
}

/// Implements [`Event`](basalt_events::Event) for a non-cancellable event struct.
///
/// `cancel()` is a no-op and `is_cancelled()` always returns `false`.
///
/// # Example
///
/// ```ignore
/// pub struct MyEvent {
///     pub data: String,
/// }
/// basalt_api::event!(MyEvent);
/// ```
#[macro_export]
macro_rules! event {
    ($name:ident) => {
        impl basalt_events::Event for $name {
            fn is_cancelled(&self) -> bool {
                false
            }
            fn cancel(&mut self) {}
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
        }
    };
}

/// A player broke a block (creative mode instant break).
///
/// Fired when the server receives a `BlockDig` packet with status 0.
/// If cancelled, the block remains unchanged and no acknowledgement
/// or broadcast is sent.
pub struct BlockBrokenEvent {
    /// Block X coordinate (absolute world coordinates).
    pub x: i32,
    /// Block Y coordinate (absolute world coordinates).
    pub y: i32,
    /// Block Z coordinate (absolute world coordinates).
    pub z: i32,
    /// Sequence number for client acknowledgement.
    pub sequence: i32,
    /// UUID of the player who broke the block.
    pub player_uuid: Uuid,
    /// Whether this event has been cancelled by a Validate handler.
    pub cancelled: bool,
}
cancellable_event!(BlockBrokenEvent);

/// A player placed a block.
///
/// Fired when the server receives a `BlockPlace` packet with a valid
/// held item that maps to a block state. The placement position has
/// already been computed from the target block + face offset.
pub struct BlockPlacedEvent {
    /// Placement X coordinate (absolute world coordinates).
    pub x: i32,
    /// Placement Y coordinate (absolute world coordinates).
    pub y: i32,
    /// Placement Z coordinate (absolute world coordinates).
    pub z: i32,
    /// The block state ID to place.
    pub block_state: u16,
    /// Sequence number for client acknowledgement.
    pub sequence: i32,
    /// UUID of the player who placed the block.
    pub player_uuid: Uuid,
    /// Whether this event has been cancelled by a Validate handler.
    pub cancelled: bool,
}
cancellable_event!(BlockPlacedEvent);

/// A player moved or changed look direction.
///
/// Fired after the player's position has been updated in the player
/// state. Carries the previous chunk coordinates for chunk boundary
/// detection. Not cancellable — the server is not authoritative for
/// position in vanilla Minecraft.
pub struct PlayerMovedEvent {
    /// The moving player's entity ID.
    pub entity_id: i32,
    /// New absolute X coordinate.
    pub x: f64,
    /// New absolute Y coordinate.
    pub y: f64,
    /// New absolute Z coordinate.
    pub z: f64,
    /// New yaw angle (degrees).
    pub yaw: f32,
    /// New pitch angle (degrees).
    pub pitch: f32,
    /// Whether the player is on the ground.
    pub on_ground: bool,
    /// Previous chunk X before the movement.
    pub old_cx: i32,
    /// Previous chunk Z before the movement.
    pub old_cz: i32,
}
event!(PlayerMovedEvent);

/// A player sent a chat message.
///
/// If cancelled, the message is not broadcast to any player.
pub struct ChatMessageEvent {
    /// The sender's username.
    pub username: String,
    /// The chat message content.
    pub message: String,
    /// Whether this event has been cancelled by a Validate handler.
    pub cancelled: bool,
}
cancellable_event!(ChatMessageEvent);

/// A player issued a command (e.g., `/tp 0 64 0`).
///
/// If cancelled, the command is not executed.
pub struct CommandEvent {
    /// The command string without the leading `/`.
    pub command: String,
    /// UUID of the player who issued the command.
    pub player_uuid: Uuid,
    /// Whether this event has been cancelled by a Validate handler.
    pub cancelled: bool,
}
cancellable_event!(CommandEvent);

/// A new player has joined the server and entered the Play state.
///
/// Not cancellable — the player is already connected.
pub struct PlayerJoinedEvent {
    /// Snapshot of the joining player's state.
    pub info: PlayerSnapshot,
}
event!(PlayerJoinedEvent);

/// A player has disconnected from the server.
///
/// Not cancellable — the connection is already closed.
pub struct PlayerLeftEvent {
    /// The leaving player's UUID.
    pub uuid: Uuid,
    /// The leaving player's entity ID.
    pub entity_id: i32,
    /// The leaving player's username.
    pub username: String,
}
event!(PlayerLeftEvent);

#[cfg(test)]
mod tests {
    use basalt_events::Event;

    use super::*;

    #[test]
    fn block_broken_cancellation() {
        let mut event = BlockBrokenEvent {
            x: 0,
            y: 64,
            z: 0,
            sequence: 1,
            player_uuid: Uuid::default(),
            cancelled: false,
        };
        assert!(!event.is_cancelled());
        event.cancel();
        assert!(event.is_cancelled());
    }

    #[test]
    fn player_moved_not_cancellable() {
        let mut event = PlayerMovedEvent {
            entity_id: 1,
            x: 0.0,
            y: 64.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: true,
            old_cx: 0,
            old_cz: 0,
        };
        event.cancel(); // no-op
        assert!(!event.is_cancelled());
    }

    #[test]
    fn event_downcast_roundtrip() {
        let mut event = BlockPlacedEvent {
            x: 5,
            y: 64,
            z: 3,
            block_state: 1,
            sequence: 42,
            player_uuid: Uuid::default(),
            cancelled: false,
        };
        let any = event.as_any_mut();
        let concrete = any.downcast_mut::<BlockPlacedEvent>().unwrap();
        assert_eq!(concrete.block_state, 1);
    }
}
