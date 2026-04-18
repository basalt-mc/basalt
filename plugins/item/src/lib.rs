//! Item plugin — manages dropped item lifecycle.
//!
//! Listens to [`BlockBrokenEvent`] in the Post stage (after the block
//! has been set to AIR by the block plugin) and spawns a dropped item
//! entity at the broken block position via [`Context::spawn_dropped_item`].
//!
//! The actual entity creation is deferred to the game loop, which
//! spawns an ECS entity with Position, Velocity, BoundingBox, Lifetime,
//! DroppedItem, and EntityKind components.

use basalt_api::prelude::*;
use basalt_api::world::block;

/// Spawns dropped item entities when blocks are broken.
///
/// Reads `BlockBrokenEvent.block_state` to determine the item to drop
/// via [`block::block_state_to_item_id`]. Non-droppable blocks (air,
/// technical blocks) produce no drop.
pub struct ItemPlugin;

impl Plugin for ItemPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "item",
            version: "0.1.0",
            author: Some("Basalt"),
            dependencies: &["block"],
        }
    }

    fn on_enable(&self, registrar: &mut PluginRegistrar) {
        registrar.on::<BlockBrokenEvent>(Stage::Post, 0, |event, ctx| {
            if let Some(item_id) = block::block_state_to_item_id(event.block_state) {
                ctx.entities().spawn_dropped_item(
                    event.position.x,
                    event.position.y,
                    event.position.z,
                    item_id,
                    1,
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use basalt_api::components::BlockPosition;
    use basalt_testkit::PluginTestHarness;

    use super::*;

    #[test]
    fn breaking_stone_produces_drop_response() {
        let mut harness = PluginTestHarness::new();
        harness.register(basalt_plugin_block::BlockPlugin);
        harness.register(ItemPlugin);

        // Place a stone block first
        harness.world().set_block(5, 64, 3, block::STONE);

        let mut event = BlockBrokenEvent {
            position: BlockPosition { x: 5, y: 64, z: 3 },
            block_state: block::STONE,
            sequence: 1,
            cancelled: false,
        };

        let responses = harness.dispatch(&mut event);

        let has_spawn = responses.iter().any(|r| {
            matches!(
                r,
                Response::SpawnDroppedItem {
                    item_id: 1,
                    count: 1,
                    ..
                }
            )
        });
        assert!(
            has_spawn,
            "breaking stone should produce a SpawnDroppedItem response"
        );
    }

    #[test]
    fn breaking_air_produces_no_drop() {
        let mut harness = PluginTestHarness::new();
        harness.register(basalt_plugin_block::BlockPlugin);
        harness.register(ItemPlugin);

        let mut event = BlockBrokenEvent {
            position: BlockPosition { x: 5, y: 64, z: 3 },
            block_state: block::AIR,
            sequence: 1,
            cancelled: false,
        };

        let responses = harness.dispatch(&mut event);

        let has_spawn = responses
            .iter()
            .any(|r| matches!(r, Response::SpawnDroppedItem { .. }));
        assert!(!has_spawn, "breaking air should not produce a drop");
    }
}
