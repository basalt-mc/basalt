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
            ctx.entities().broadcast_entity_moved(
                ctx.player().entity_id(),
                event.position.x,
                event.position.y,
                event.position.z,
                event.rotation.yaw,
                event.rotation.pitch,
                event.on_ground,
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use basalt_api::Response;
    use basalt_api::components::{ChunkPosition, Position, Rotation};
    use basalt_testkit::PluginTestHarness;

    use super::*;

    #[test]
    fn movement_broadcasts() {
        let mut harness = PluginTestHarness::new();
        harness.register(MovementPlugin);

        let mut event = PlayerMovedEvent {
            position: Position {
                x: 10.0,
                y: 64.0,
                z: 5.0,
            },
            rotation: Rotation {
                yaw: 90.0,
                pitch: 0.0,
            },
            on_ground: true,
            old_chunk: ChunkPosition { x: 0, z: 0 },
        };

        let responses = harness.dispatch(&mut event);
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::Broadcast(BroadcastMessage::EntityMoved { .. })
        ));
    }
}
