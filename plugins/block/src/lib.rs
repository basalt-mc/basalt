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
            ctx.world_ctx().set_block(
                event.position.x,
                event.position.y,
                event.position.z,
                basalt_api::world::block::AIR,
            );
        });

        registrar.on::<BlockPlacedEvent>(Stage::Process, 0, |event, ctx| {
            ctx.world_ctx().set_block(
                event.position.x,
                event.position.y,
                event.position.z,
                event.block_state,
            );
        });

        // Post: acknowledge + broadcast
        registrar.on::<BlockBrokenEvent>(Stage::Post, 0, |event, ctx| {
            ctx.world_ctx().send_block_ack(event.sequence);
            ctx.entities().broadcast_block_change(
                event.position.x,
                event.position.y,
                event.position.z,
                basalt_api::world::block::AIR as i32,
            );
        });

        registrar.on::<BlockPlacedEvent>(Stage::Post, 0, |event, ctx| {
            ctx.world_ctx().send_block_ack(event.sequence);
            ctx.entities().broadcast_block_change(
                event.position.x,
                event.position.y,
                event.position.z,
                event.block_state as i32,
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use basalt_api::components::BlockPosition;
    use basalt_api::testing::PluginTestHarness;

    use super::*;

    #[test]
    fn block_broken_sets_air_and_queues_responses() {
        let mut harness = PluginTestHarness::new();
        harness
            .world()
            .set_block(5, 64, 3, basalt_api::world::block::STONE);
        harness.register(BlockPlugin);

        let mut event = BlockBrokenEvent {
            position: BlockPosition { x: 5, y: 64, z: 3 },
            block_state: basalt_api::world::block::STONE,
            sequence: 42,
            cancelled: false,
        };

        let result = harness.dispatch(&mut event);

        assert_eq!(
            harness.world().get_block(5, 64, 3),
            basalt_api::world::block::AIR
        );
        assert_eq!(result.len(), 2);
        assert!(result.has_block_ack_seq(42));
        assert!(result.has_block_change_broadcast());
    }

    #[test]
    fn cancelled_block_break_skips_mutation() {
        let mut harness = PluginTestHarness::new();
        harness
            .world()
            .set_block(8, 64, 8, basalt_api::world::block::STONE);

        // Register a Validate handler that cancels before BlockPlugin runs
        harness.on::<BlockBrokenEvent>(Stage::Validate, 0, |event, _ctx| {
            event.cancel();
        });
        harness.register(BlockPlugin);

        let mut event = BlockBrokenEvent {
            position: BlockPosition { x: 8, y: 64, z: 8 },
            block_state: basalt_api::world::block::STONE,
            sequence: 1,
            cancelled: false,
        };

        let result = harness.dispatch(&mut event);

        assert_eq!(
            harness.world().get_block(8, 64, 8),
            basalt_api::world::block::STONE
        );
        assert!(result.is_empty());
    }

    #[test]
    fn block_placed_sets_state_and_queues_responses() {
        let mut harness = PluginTestHarness::new();
        harness.register(BlockPlugin);

        let mut event = BlockPlacedEvent {
            position: BlockPosition { x: 5, y: 64, z: 3 },
            block_state: basalt_api::world::block::STONE,
            sequence: 10,
            cancelled: false,
        };

        let result = harness.dispatch(&mut event);

        assert_eq!(
            harness.world().get_block(5, 64, 3),
            basalt_api::world::block::STONE
        );
        assert_eq!(result.len(), 2);
        assert!(result.has_block_ack_seq(10));
        assert!(result.has_block_change_broadcast());
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
            position: BlockPosition { x: 5, y: 200, z: 3 },
            block_state: basalt_api::world::block::STONE,
            sequence: 10,
            cancelled: false,
        };

        let result = harness.dispatch(&mut event);
        assert_eq!(
            harness.world().get_block(5, 200, 3),
            basalt_api::world::block::AIR
        );
        assert!(result.is_empty());
    }
}
