//! Block interaction events: breaking, placing, right-click.

use crate::components::BlockPosition;

/// A player broke a block.
///
/// Fired when the server receives a `BlockDig` packet with status 0.
/// The breaking player is available via `ctx.player()`.
#[derive(Debug, Clone)]
pub struct BlockBrokenEvent {
    /// Position of the broken block.
    pub position: BlockPosition,
    /// Block state that was at this position before breaking.
    pub block_state: u16,
    /// Sequence number for client acknowledgement.
    pub sequence: i32,
    /// Whether this event has been cancelled by a Validate handler.
    pub cancelled: bool,
}
crate::game_cancellable_event!(BlockBrokenEvent);

/// A player placed a block.
///
/// Fired when the server receives a `BlockPlace` packet with a valid
/// held item. The placement position has already been computed from
/// the target block + face offset. The placing player is available
/// via `ctx.player()`.
#[derive(Debug, Clone)]
pub struct BlockPlacedEvent {
    /// Position where the block was placed.
    pub position: BlockPosition,
    /// The block state ID that was placed.
    pub block_state: u16,
    /// Sequence number for client acknowledgement.
    pub sequence: i32,
    /// Whether this event has been cancelled by a Validate handler.
    pub cancelled: bool,
}
crate::game_cancellable_event!(BlockPlacedEvent);

/// A player right-clicked on a block.
///
/// Fired before any container interaction or block placement.
/// If cancelled (e.g., ContainerPlugin opens a chest), the game
/// loop skips block placement. The interacting player is available
/// via `ctx.player()`.
#[derive(Debug, Clone)]
pub struct PlayerInteractEvent {
    /// Position of the clicked block.
    pub position: BlockPosition,
    /// Block state at the clicked position.
    pub block_state: u16,
    /// Face direction clicked (0-5).
    pub direction: i32,
    /// Sequence number for acknowledgement.
    pub sequence: i32,
    /// Whether this event has been cancelled.
    pub cancelled: bool,
}
crate::game_cancellable_event!(PlayerInteractEvent);

#[cfg(test)]
mod tests {
    use crate::events::Event;

    use super::*;

    #[test]
    fn block_broken_cancellation() {
        let mut event = BlockBrokenEvent {
            position: BlockPosition { x: 0, y: 64, z: 0 },
            block_state: 1,
            sequence: 1,
            cancelled: false,
        };
        assert!(!event.is_cancelled());
        event.cancel();
        assert!(event.is_cancelled());
    }

    #[test]
    fn block_placed_downcast_roundtrip() {
        let mut event = BlockPlacedEvent {
            position: BlockPosition { x: 5, y: 64, z: 3 },
            block_state: 1,
            sequence: 42,
            cancelled: false,
        };
        let any = event.as_any_mut();
        let concrete = any.downcast_mut::<BlockPlacedEvent>().unwrap();
        assert_eq!(concrete.block_state, 1);
    }

    #[test]
    fn event_routing() {
        use crate::events::{BusKind, EventRouting};
        assert_eq!(BlockBrokenEvent::BUS, BusKind::Game);
        assert_eq!(BlockPlacedEvent::BUS, BusKind::Game);
        assert_eq!(PlayerInteractEvent::BUS, BusKind::Game);
    }
}
