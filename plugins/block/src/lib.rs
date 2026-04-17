//! Block interaction plugin.
//!
//! Handles block breaking and placing: mutates the world in the
//! Process stage, then queues acknowledgement and broadcast in Post.

use basalt_api::prelude::*;

/// Handles block breaking and placing in the world.
///
/// - **Process**: sets the block in the world (AIR for break, block_state for place)
/// - **Post**: sends acknowledgement and broadcasts the change
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
            ctx.world()
                .set_block(event.x, event.y, event.z, basalt_world::block::AIR);
        });

        registrar.on::<BlockPlacedEvent>(Stage::Process, 0, |event, ctx| {
            ctx.world()
                .set_block(event.x, event.y, event.z, event.block_state);
        });

        // Post: acknowledge + broadcast
        registrar.on::<BlockBrokenEvent>(Stage::Post, 0, |event, ctx| {
            ctx.send_block_ack(event.sequence);
            ctx.broadcast(BroadcastMessage::BlockChanged {
                x: event.x,
                y: event.y,
                z: event.z,
                block_state: basalt_world::block::AIR as i32,
            });
        });

        registrar.on::<BlockPlacedEvent>(Stage::Post, 0, |event, ctx| {
            ctx.send_block_ack(event.sequence);
            ctx.broadcast(BroadcastMessage::BlockChanged {
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
    use basalt_api::context::ServerContext;
    use basalt_api::{Event, EventBus, Response};
    use basalt_test_utils::PluginTestHarness;
    use basalt_types::Uuid;

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
            sequence: 42,
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let ctx = harness.context();
        let responses = harness.dispatch_with(&mut event, &ctx);

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
        let world = std::sync::Arc::new(basalt_world::World::new_memory(42));
        world.set_block(8, 64, 8, basalt_world::block::STONE);

        let ctx = ServerContext::new(world.clone(), Uuid::default(), 1, "Steve".into(), 0.0, 0.0);
        let mut event = BlockBrokenEvent {
            x: 8,
            y: 64,
            z: 8,
            sequence: 1,
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let mut instant_bus = EventBus::new();
        let mut game_bus = EventBus::new();
        // Validate handler cancels before BlockPlugin runs
        let mut cmds = Vec::new();
        let mut systems = Vec::new();
        let mut components = Vec::new();
        let mut registrar = basalt_api::plugin::PluginRegistrar::new(
            &mut instant_bus,
            &mut game_bus,
            &mut cmds,
            &mut systems,
            &mut components,
            std::sync::Arc::new(basalt_world::World::new_memory(42)),
        );
        registrar.on::<BlockBrokenEvent>(Stage::Validate, 0, |event, _| {
            event.cancel();
        });
        BlockPlugin.on_enable(&mut registrar);
        game_bus.dispatch(&mut event, &ctx);

        assert_eq!(world.get_block(8, 64, 8), basalt_world::block::STONE);
        assert!(ctx.drain_responses().is_empty());
    }
}
