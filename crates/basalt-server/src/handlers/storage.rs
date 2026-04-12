//! Storage handler plugin.
//!
//! Persists modified chunks to disk after block changes. When this
//! plugin is disabled, block changes exist only in memory and are
//! lost on server restart — useful for lobby servers with read-only
//! worlds.

use basalt_events::{EventBus, Stage};

use crate::context::EventContext;
use crate::events::{BlockBrokenEvent, BlockPlacedEvent};

/// Persists chunks to disk after block modifications.
///
/// - **Post BlockBrokenEvent**: persists the affected chunk
/// - **Post BlockPlacedEvent**: persists the affected chunk
///
/// Disabling this handler means no disk I/O on block changes.
pub struct StorageHandler;

impl StorageHandler {
    /// Registers storage handlers on the event bus.
    pub fn register(bus: &mut EventBus) {
        bus.on::<BlockBrokenEvent, EventContext>(Stage::Post, 10, |event, ctx| {
            let cx = event.x >> 4;
            let cz = event.z >> 4;
            ctx.state.world.persist_chunk(cx, cz);
        });

        bus.on::<BlockPlacedEvent, EventContext>(Stage::Post, 10, |event, ctx| {
            let cx = event.x >> 4;
            let cz = event.z >> 4;
            ctx.state.world.persist_chunk(cx, cz);
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use basalt_types::Uuid;

    use super::*;
    use crate::handlers::BlockInteractionHandler;
    use crate::state::ServerState;

    #[test]
    fn block_change_persists_with_storage_handler() {
        let dir = tempfile::tempdir().unwrap();

        // Create a server state with disk persistence
        let state = ServerState::new_with_world(basalt_world::World::new(42, dir.path()));
        let ctx = EventContext::new(Arc::clone(&state));

        let mut event = BlockPlacedEvent {
            x: 5,
            y: 100,
            z: 3,
            block_state: basalt_world::block::STONE,
            sequence: 1,
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        BlockInteractionHandler::register(&mut bus);
        StorageHandler::register(&mut bus);
        bus.dispatch(&mut event, &ctx);

        // Verify it was persisted — load from a fresh world
        let world2 = basalt_world::World::new(42, dir.path());
        assert_eq!(world2.get_block(5, 100, 3), basalt_world::block::STONE);
    }

    #[test]
    fn block_change_without_storage_handler_is_memory_only() {
        let dir = tempfile::tempdir().unwrap();

        let state = ServerState::new_with_world(basalt_world::World::new(42, dir.path()));
        let ctx = EventContext::new(Arc::clone(&state));

        let mut event = BlockPlacedEvent {
            x: 5,
            y: 100,
            z: 3,
            block_state: basalt_world::block::STONE,
            sequence: 1,
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        // Only BlockInteractionHandler, no StorageHandler
        let mut bus = EventBus::new();
        BlockInteractionHandler::register(&mut bus);
        bus.dispatch(&mut event, &ctx);

        // Block is in memory
        assert_eq!(state.world.get_block(5, 100, 3), basalt_world::block::STONE);

        // But NOT on disk
        let world2 = basalt_world::World::new(42, dir.path());
        assert_ne!(world2.get_block(5, 100, 3), basalt_world::block::STONE);
    }
}
