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
    use basalt_types::Uuid;

    use super::*;

    fn test_world() -> &'static basalt_world::World {
        use std::sync::OnceLock;
        static WORLD: OnceLock<basalt_world::World> = OnceLock::new();
        WORLD.get_or_init(|| basalt_world::World::new_memory(42))
    }

    #[test]
    fn block_broken_sets_air_and_queues_responses() {
        test_world().set_block(5, 64, 3, basalt_world::block::STONE);

        let ctx = ServerContext::new(test_world(), Uuid::default(), 1, "Steve".into());
        let mut event = BlockBrokenEvent {
            x: 5,
            y: 64,
            z: 3,
            sequence: 42,
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        let mut cmds = Vec::new();
        let mut registrar = PluginRegistrar::new(&mut bus, &mut cmds);
        BlockPlugin.on_enable(&mut registrar);
        bus.dispatch(&mut event, &ctx);

        assert_eq!(test_world().get_block(5, 64, 3), basalt_world::block::AIR);

        let responses = ctx.drain_responses();
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
        test_world().set_block(8, 64, 8, basalt_world::block::STONE);

        let ctx = ServerContext::new(test_world(), Uuid::default(), 1, "Steve".into());
        let mut event = BlockBrokenEvent {
            x: 8,
            y: 64,
            z: 8,
            sequence: 1,
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        // Validate handler cancels
        let mut cmds = Vec::new();
        let mut registrar = PluginRegistrar::new(&mut bus, &mut cmds);
        registrar.on::<BlockBrokenEvent>(Stage::Validate, 0, |event, _| {
            event.cancel();
        });
        BlockPlugin.on_enable(&mut registrar);
        bus.dispatch(&mut event, &ctx);

        assert_eq!(test_world().get_block(8, 64, 8), basalt_world::block::STONE);
        assert!(ctx.drain_responses().is_empty());
    }
}
