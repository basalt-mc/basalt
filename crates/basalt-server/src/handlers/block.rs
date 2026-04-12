//! Block interaction handler plugin.
//!
//! Handles block breaking and placing: mutates the world in the
//! Process stage, then queues acknowledgement and broadcast in Post.

use basalt_events::{EventBus, Stage};

use crate::context::{EventContext, Response};
use crate::events::{BlockBrokenEvent, BlockPlacedEvent};
use crate::state::BroadcastMessage;

/// Handles block breaking and placing in the world.
///
/// - **Process**: sets the block in the world (AIR for break, block_state for place)
/// - **Post**: sends acknowledgement to the player and broadcasts the change
pub struct BlockInteractionHandler;

impl BlockInteractionHandler {
    /// Registers block interaction handlers on the event bus.
    pub fn register(bus: &mut EventBus) {
        // Process: mutate world state
        bus.on::<BlockBrokenEvent, EventContext>(Stage::Process, 0, |event, ctx| {
            ctx.state
                .world
                .set_block(event.x, event.y, event.z, basalt_world::block::AIR);
        });

        bus.on::<BlockPlacedEvent, EventContext>(Stage::Process, 0, |event, ctx| {
            ctx.state
                .world
                .set_block(event.x, event.y, event.z, event.block_state);
        });

        // Post: acknowledge + broadcast
        bus.on::<BlockBrokenEvent, EventContext>(Stage::Post, 0, |event, ctx| {
            ctx.responses.push(Response::SendBlockAck {
                sequence: event.sequence,
            });
            ctx.responses
                .push(Response::Broadcast(BroadcastMessage::BlockChanged {
                    x: event.x,
                    y: event.y,
                    z: event.z,
                    block_state: basalt_world::block::AIR as i32,
                }));
        });

        bus.on::<BlockPlacedEvent, EventContext>(Stage::Post, 0, |event, ctx| {
            ctx.responses.push(Response::SendBlockAck {
                sequence: event.sequence,
            });
            ctx.responses
                .push(Response::Broadcast(BroadcastMessage::BlockChanged {
                    x: event.x,
                    y: event.y,
                    z: event.z,
                    block_state: event.block_state as i32,
                }));
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use basalt_events::Event;
    use basalt_types::Uuid;

    use super::*;
    use crate::state::ServerState;

    #[test]
    fn block_broken_sets_air_and_queues_responses() {
        let state = ServerState::new();
        state.world.set_block(5, 64, 3, basalt_world::block::STONE);

        let ctx = EventContext::new(Arc::clone(&state));
        let mut event = BlockBrokenEvent {
            x: 5,
            y: 64,
            z: 3,
            sequence: 42,
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        BlockInteractionHandler::register(&mut bus);
        bus.dispatch(&mut event, &ctx);

        assert_eq!(state.world.get_block(5, 64, 3), basalt_world::block::AIR);

        let responses = ctx.responses.drain();
        assert_eq!(responses.len(), 2);
        assert!(matches!(
            responses[0],
            Response::SendBlockAck { sequence: 42 }
        ));
        assert!(matches!(
            responses[1],
            Response::Broadcast(BroadcastMessage::BlockChanged {
                x: 5,
                y: 64,
                z: 3,
                ..
            })
        ));
    }

    #[test]
    fn block_placed_sets_block_and_queues_responses() {
        let state = ServerState::new();
        let ctx = EventContext::new(Arc::clone(&state));
        let mut event = BlockPlacedEvent {
            x: 10,
            y: 65,
            z: 10,
            block_state: basalt_world::block::STONE,
            sequence: 7,
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        BlockInteractionHandler::register(&mut bus);
        bus.dispatch(&mut event, &ctx);

        assert_eq!(
            state.world.get_block(10, 65, 10),
            basalt_world::block::STONE
        );

        let responses = ctx.responses.drain();
        assert_eq!(responses.len(), 2);
    }

    #[test]
    fn cancelled_block_break_skips_mutation() {
        let state = ServerState::new();
        state.world.set_block(5, 64, 3, basalt_world::block::STONE);

        let ctx = EventContext::new(Arc::clone(&state));
        let mut event = BlockBrokenEvent {
            x: 5,
            y: 64,
            z: 3,
            sequence: 1,
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        // Validate handler cancels
        bus.on::<BlockBrokenEvent, EventContext>(Stage::Validate, 0, |event, _| {
            event.cancel();
        });
        BlockInteractionHandler::register(&mut bus);
        bus.dispatch(&mut event, &ctx);

        // Block should NOT have changed
        assert_eq!(state.world.get_block(5, 64, 3), basalt_world::block::STONE);
        assert!(ctx.responses.drain().is_empty());
    }
}
