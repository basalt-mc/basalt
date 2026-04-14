//! Movement broadcast plugin.
//!
//! Broadcasts player position and look updates to other connected
//! players. Without this plugin, players cannot see each other move.

use basalt_api::prelude::*;

/// Broadcasts player movement to other connected players.
///
/// - **Post PlayerMovedEvent**: broadcasts position/look via EntityMoved
pub struct MovementPlugin;

impl Plugin for MovementPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "movement",
            version: "0.1.0",
            author: Some("Basalt"),
            dependencies: &[],
        }
    }

    fn on_enable(&self, registrar: &mut PluginRegistrar) {
        registrar.on::<PlayerMovedEvent>(Stage::Post, 0, |event, ctx| {
            ctx.broadcast(BroadcastMessage::EntityMoved {
                entity_id: event.entity_id,
                x: event.x,
                y: event.y,
                z: event.z,
                yaw: event.yaw,
                pitch: event.pitch,
                on_ground: event.on_ground,
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use basalt_api::context::ServerContext;
    use basalt_api::{EventBus, Response};
    use basalt_types::Uuid;

    use super::*;

    fn test_world() -> &'static basalt_world::World {
        use std::sync::OnceLock;
        static WORLD: OnceLock<basalt_world::World> = OnceLock::new();
        WORLD.get_or_init(|| basalt_world::World::new_memory(42))
    }

    #[test]
    fn movement_broadcasts() {
        let ctx = ServerContext::new(test_world(), Uuid::default(), 1, "Steve".into());
        let mut event = PlayerMovedEvent {
            entity_id: 1,
            x: 10.0,
            y: 64.0,
            z: 5.0,
            yaw: 90.0,
            pitch: 0.0,
            on_ground: true,
            old_cx: 0,
            old_cz: 0,
        };

        let mut bus = EventBus::new();
        let mut cmds = Vec::new();
        let mut registrar = PluginRegistrar::new(&mut bus, &mut cmds);
        MovementPlugin.on_enable(&mut registrar);
        bus.dispatch(&mut event, &ctx);

        let responses = ctx.drain_responses();
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::Broadcast(BroadcastMessage::EntityMoved { .. })
        ));
    }
}
