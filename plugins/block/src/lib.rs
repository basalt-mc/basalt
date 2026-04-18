//! Block interaction plugin.
//!
//! Handles block breaking and placing: mutates the world in the
//! Process stage, then queues acknowledgement and broadcast in Post.

use basalt_api::prelude::*;

/// Handles block breaking and placing in the world.
pub struct BlockPlugin;

impl Plugin for BlockPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "block",
            version: "0.1.0",
            author: Some("Basalt"),
            dependencies: &[],
        }
    }

    fn on_enable(&self, registrar: &mut PluginRegistrar) {
        // Process: mutate world state
        registrar.on::<BlockBrokenEvent>(Stage::Process, 0, |event, ctx| {
            ctx.world_ctx()
                .world()
                .set_block(event.x, event.y, event.z, basalt_world::block::AIR);
        });

        registrar.on::<BlockPlacedEvent>(Stage::Process, 0, |event, ctx| {
            ctx.world_ctx()
                .world()
                .set_block(event.x, event.y, event.z, event.block_state);
        });

        // Post: acknowledge + broadcast
        registrar.on::<BlockBrokenEvent>(Stage::Post, 0, |event, ctx| {
            ctx.world_ctx().send_block_ack(event.sequence);
            ctx.entities()
                .broadcast_raw(BroadcastMessage::BlockChanged {
                    x: event.x,
                    y: event.y,
                    z: event.z,
                    block_state: basalt_world::block::AIR as i32,
                });
        });

        registrar.on::<BlockPlacedEvent>(Stage::Post, 0, |event, ctx| {
            ctx.world_ctx().send_block_ack(event.sequence);
            ctx.entities()
                .broadcast_raw(BroadcastMessage::BlockChanged {
                    x: event.x,
                    y: event.y,
                    z: event.z,
                    block_state: event.block_state as i32,
                });
        });
    }
}

#[cfg(test)]
mod tests {
    use basalt_testkit::PluginTestHarness;

    use super::*;

    #[test]
    fn block_broken_sets_air_and_queues_responses() {
        let mut harness = PluginTestHarness::new();
        harness
            .world()
            .set_block(5, 64, 3, basalt_world::block::STONE);
        harness.register(BlockPlugin);

        let mut event = BlockBrokenEvent {
            x: 5,
            y: 64,
            z: 3,
            block_state: basalt_world::block::STONE,
            sequence: 42,
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let responses = harness.dispatch(&mut event);

        assert_eq!(
            harness.world().get_block(5, 64, 3),
            basalt_world::block::AIR
        );
        assert_eq!(responses.len(), 2);
        assert!(matches!(
            responses[0],
            Response::SendBlockAck { sequence: 42 }
        ));
        assert!(matches!(
            responses[1],
            Response::Broadcast(BroadcastMessage::BlockChanged { .. })
        ));
    }

    #[test]
    fn cancelled_block_break_skips_mutation() {
        let mut harness = PluginTestHarness::new();
        harness
            .world()
            .set_block(8, 64, 8, basalt_world::block::STONE);

        // Register a Validate handler that cancels before BlockPlugin runs
        harness.on::<BlockBrokenEvent>(Stage::Validate, 0, |event, _ctx| {
            event.cancel();
        });
        harness.register(BlockPlugin);

        let mut event = BlockBrokenEvent {
            x: 8,
            y: 64,
            z: 8,
            block_state: basalt_world::block::STONE,
            sequence: 1,
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let responses = harness.dispatch(&mut event);

        assert_eq!(
            harness.world().get_block(8, 64, 8),
            basalt_world::block::STONE
        );
        assert!(responses.is_empty());
    }

    #[test]
    fn block_placed_sets_state_and_queues_responses() {
        let mut harness = PluginTestHarness::new();
        harness.register(BlockPlugin);

        let mut event = BlockPlacedEvent {
            x: 5,
            y: 64,
            z: 3,
            block_state: basalt_world::block::STONE,
            sequence: 10,
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let responses = harness.dispatch(&mut event);

        assert_eq!(
            harness.world().get_block(5, 64, 3),
            basalt_world::block::STONE
        );
        assert_eq!(responses.len(), 2);
        assert!(matches!(
            responses[0],
            Response::SendBlockAck { sequence: 10 }
        ));
        assert!(matches!(
            responses[1],
            Response::Broadcast(BroadcastMessage::BlockChanged { .. })
        ));
    }

    #[test]
    fn cancelled_block_place_skips_mutation() {
        let mut harness = PluginTestHarness::new();
        harness.on::<BlockPlacedEvent>(Stage::Validate, 0, |event, _ctx| {
            event.cancel();
        });
        harness.register(BlockPlugin);

        // Use y=200 which is guaranteed to be air in any world
        let mut event = BlockPlacedEvent {
            x: 5,
            y: 200,
            z: 3,
            block_state: basalt_world::block::STONE,
            sequence: 10,
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let responses = harness.dispatch(&mut event);
        assert_eq!(
            harness.world().get_block(5, 200, 3),
            basalt_world::block::AIR
        );
        assert!(responses.is_empty());
    }
}
