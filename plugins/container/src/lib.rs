//! Container plugin — chest interaction, double chest pairing, block entities.
//!
//! Handles all container-related game logic via events:
//! - [`PlayerInteractEvent`]: opens chests on right-click (cancels to prevent block placement)
//! - [`BlockPlacedEvent`]: creates chest block entities, handles double chest pairing and orientation
//! - [`BlockBrokenEvent`]: drops chest contents, removes block entities, reverts double chests

use basalt_api::prelude::*;
use basalt_world::block;
use basalt_world::block_entity::BlockEntity;

/// Handles chest interaction, double chest pairing, and block entity lifecycle.
pub struct ContainerPlugin;

impl Plugin for ContainerPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "container",
            version: "0.1.0",
            author: Some("Basalt"),
            dependencies: &["block"],
        }
    }

    fn on_enable(&self, registrar: &mut PluginRegistrar) {
        // Open chest on right-click (Process stage, cancels event)
        registrar.on::<PlayerInteractEvent>(Stage::Process, 0, |event, ctx| {
            if block::is_chest(event.block_state) {
                ctx.containers().open_chest(event.x, event.y, event.z);
                event.cancel();
            }
        });

        // Create block entity + handle double chest on placement (Post stage)
        registrar.on::<BlockPlacedEvent>(Stage::Post, 5, |event, ctx| {
            let state = event.block_state;
            if !block::is_chest(state) {
                return;
            }

            let world = ctx.world_ctx().world();

            // Create block entity
            world.set_block_entity(event.x, event.y, event.z, BlockEntity::empty_chest());

            // Orient chest based on player yaw
            let yaw = ctx.player().yaw();
            let oriented = block::chest_state_for_yaw(yaw);
            world.set_block(event.x, event.y, event.z, oriented);
            ctx.entities()
                .broadcast_raw(BroadcastMessage::BlockChanged {
                    x: event.x,
                    y: event.y,
                    z: event.z,
                    block_state: i32::from(oriented),
                });

            // Double chest pairing — scan adjacent for single chest with same facing
            let facing = block::chest_facing(oriented);
            let offsets = block::chest_adjacent_offsets(facing);
            for &(dx, dz) in &offsets {
                let nx = event.x + dx;
                let nz = event.z + dz;
                let neighbor = world.get_block(nx, event.y, nz);
                if block::is_single_chest(neighbor) && block::chest_facing(neighbor) == facing {
                    let ddx = nx - event.x;
                    let ddz = nz - event.z;
                    let (new_type, existing_type) = block::chest_double_types(facing, ddx, ddz);
                    let new_state = block::chest_state(facing, new_type);
                    let neighbor_state = block::chest_state(facing, existing_type);
                    world.set_block(event.x, event.y, event.z, new_state);
                    world.set_block(nx, event.y, nz, neighbor_state);
                    world.mark_chunk_dirty(event.x >> 4, event.z >> 4);
                    world.mark_chunk_dirty(nx >> 4, nz >> 4);
                    ctx.entities()
                        .broadcast_raw(BroadcastMessage::BlockChanged {
                            x: event.x,
                            y: event.y,
                            z: event.z,
                            block_state: i32::from(new_state),
                        });
                    ctx.entities()
                        .broadcast_raw(BroadcastMessage::BlockChanged {
                            x: nx,
                            y: event.y,
                            z: nz,
                            block_state: i32::from(neighbor_state),
                        });
                    break;
                }
            }
        });

        // Drop chest contents + remove block entity on break (Post, before drops plugin)
        registrar.on::<BlockBrokenEvent>(Stage::Post, -1, |event, ctx| {
            let state = event.block_state;
            if !block::is_chest(state) {
                return;
            }

            let world = ctx.world_ctx().world();

            // Drop contents
            if let Some(be) = world.get_block_entity(event.x, event.y, event.z) {
                match &*be {
                    BlockEntity::Chest { slots } => {
                        for slot in slots.iter() {
                            if let Some(item_id) = slot.item_id {
                                ctx.entities().spawn_dropped_item(
                                    event.x,
                                    event.y,
                                    event.z,
                                    item_id,
                                    slot.item_count,
                                );
                            }
                        }
                    }
                }
            }

            // Remove block entity
            world.remove_block_entity(event.x, event.y, event.z);

            // Revert double chest partner to single
            if block::chest_type(state) != 0 {
                let facing = block::chest_facing(state);
                let offsets = block::chest_adjacent_offsets(facing);
                for &(dx, dz) in &offsets {
                    let nx = event.x + dx;
                    let nz = event.z + dz;
                    let neighbor = world.get_block(nx, event.y, nz);
                    if block::is_chest(neighbor)
                        && block::chest_facing(neighbor) == facing
                        && block::chest_type(neighbor) != 0
                    {
                        let single = block::chest_state(facing, 0);
                        world.set_block(nx, event.y, nz, single);
                        world.mark_chunk_dirty(nx >> 4, nz >> 4);
                        ctx.entities()
                            .broadcast_raw(BroadcastMessage::BlockChanged {
                                x: nx,
                                y: event.y,
                                z: nz,
                                block_state: i32::from(single),
                            });
                        break;
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use basalt_testkit::PluginTestHarness;
    use basalt_types::Uuid;

    use super::*;

    #[test]
    fn interact_on_chest_cancels_event() {
        let mut harness = PluginTestHarness::new();
        harness.register(ContainerPlugin);

        let mut event = PlayerInteractEvent {
            x: 5,
            y: 64,
            z: 3,
            player_uuid: Uuid::default(),
            block_state: block::CHEST,
            direction: 1,
            sequence: 1,
            cancelled: false,
        };

        harness.dispatch(&mut event);
        assert!(event.is_cancelled(), "interact on chest should cancel");
    }

    #[test]
    fn interact_on_non_chest_does_not_cancel() {
        let mut harness = PluginTestHarness::new();
        harness.register(ContainerPlugin);

        let mut event = PlayerInteractEvent {
            x: 5,
            y: 64,
            z: 3,
            player_uuid: Uuid::default(),
            block_state: block::STONE,
            direction: 1,
            sequence: 1,
            cancelled: false,
        };

        harness.dispatch(&mut event);
        assert!(!event.is_cancelled(), "interact on stone should not cancel");
    }

    #[test]
    fn chest_break_removes_block_entity() {
        let mut harness = PluginTestHarness::new();
        harness.register(basalt_plugin_block::BlockPlugin);
        harness.register(ContainerPlugin);

        // Place a chest block entity manually
        harness.world().set_block(5, 64, 3, block::CHEST);
        harness
            .world()
            .set_block_entity(5, 64, 3, BlockEntity::empty_chest());

        let mut event = BlockBrokenEvent {
            x: 5,
            y: 64,
            z: 3,
            block_state: block::CHEST,
            sequence: 1,
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        harness.dispatch(&mut event);
        assert!(
            harness.world().get_block_entity(5, 64, 3).is_none(),
            "block entity should be removed"
        );
    }
}
