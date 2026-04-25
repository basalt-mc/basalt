//! Container plugin — chest interaction, double chest pairing, block entities.
//!
//! All container behaviour is event-driven:
//! - [`PlayerInteractEvent`] opens chests on right-click.
//! - [`BlockPlacedEvent`] creates the chest block entity, orients it
//!   to the player and pairs adjacent halves into a double chest.
//! - [`BlockBrokenEvent`] reverts double-chest partners and triggers
//!   block-entity destruction through the event pipeline.
//! - [`ContainerOpenedEvent`] / [`ContainerClosedEvent`] broadcast the
//!   chest-lid open/close animation with the right viewer count.
//! - [`ContainerSlotChangedEvent`] keeps co-viewers' open chests in
//!   sync.
//! - [`BlockEntityDestroyedEvent`] drops chest contents as item
//!   entities (split out of the legacy inline `BlockBrokenEvent`
//!   path).

use basalt_api::prelude::*;
use basalt_api::world::block;
use basalt_api::world::block_entity::BlockEntity;

/// Block registry ID for `chest` in 1.21.4.
///
/// Used as the `block_id` field of the `BlockAction` packet driving
/// the chest-lid open/close animation. See the protocol wiki entry
/// for "Block Action" — for chests, `action_id = 1` and
/// `action_param` is the current number of viewers (0 = closed lid).
const CHEST_BLOCK_ID: i32 = 185;

/// Returns up to two block positions covering a chest at `(x, y, z)`.
///
/// Single chests return one position; double chests return both
/// halves so the lid animation can play on each. Falls back to a
/// single position if the world doesn't currently report a chest at
/// `(x, y, z)` (e.g. the block was already broken).
fn chest_parts(
    world_ctx: &dyn basalt_api::context::WorldContext,
    x: i32,
    y: i32,
    z: i32,
) -> Vec<(i32, i32, i32)> {
    let mut parts: Vec<(i32, i32, i32)> = Vec::with_capacity(2);
    parts.push((x, y, z));
    let state = world_ctx.get_block(x, y, z);
    if !block::is_chest(state) || block::chest_type(state) == 0 {
        return parts;
    }
    let facing = block::chest_facing(state);
    for &(dx, dz) in &block::chest_adjacent_offsets(facing) {
        let nx = x + dx;
        let nz = z + dz;
        let neighbor = world_ctx.get_block(nx, y, nz);
        if block::is_chest(neighbor) && block::chest_facing(neighbor) == facing {
            parts.push((nx, y, nz));
            break;
        }
    }
    parts
}

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
                ctx.containers()
                    .open_chest(event.position.x, event.position.y, event.position.z);
                event.cancel();
            }
        });

        // Create block entity + handle double chest on placement (Post stage)
        registrar.on::<BlockPlacedEvent>(Stage::Post, 5, |event, ctx| {
            let state = event.block_state;
            if !block::is_chest(state) {
                return;
            }

            let wctx = ctx.world_ctx();

            // Create block entity
            wctx.set_block_entity(
                event.position.x,
                event.position.y,
                event.position.z,
                BlockEntity::empty_chest(),
            );

            // Orient chest based on player yaw
            let yaw = ctx.player().yaw();
            let oriented = block::chest_state_for_yaw(yaw);
            wctx.set_block(
                event.position.x,
                event.position.y,
                event.position.z,
                oriented,
            );
            ctx.entities().broadcast_block_change(
                event.position.x,
                event.position.y,
                event.position.z,
                i32::from(oriented),
            );

            // Double chest pairing — scan adjacent for single chest with same facing
            let facing = block::chest_facing(oriented);
            let offsets = block::chest_adjacent_offsets(facing);
            for &(dx, dz) in &offsets {
                let nx = event.position.x + dx;
                let nz = event.position.z + dz;
                let neighbor = wctx.get_block(nx, event.position.y, nz);
                if block::is_single_chest(neighbor) && block::chest_facing(neighbor) == facing {
                    let ddx = nx - event.position.x;
                    let ddz = nz - event.position.z;
                    let (new_type, existing_type) = block::chest_double_types(facing, ddx, ddz);
                    let new_state = block::chest_state(facing, new_type);
                    let neighbor_state = block::chest_state(facing, existing_type);
                    wctx.set_block(
                        event.position.x,
                        event.position.y,
                        event.position.z,
                        new_state,
                    );
                    wctx.set_block(nx, event.position.y, nz, neighbor_state);
                    wctx.mark_chunk_dirty(event.position.x >> 4, event.position.z >> 4);
                    wctx.mark_chunk_dirty(nx >> 4, nz >> 4);
                    ctx.entities().broadcast_block_change(
                        event.position.x,
                        event.position.y,
                        event.position.z,
                        i32::from(new_state),
                    );
                    ctx.entities().broadcast_block_change(
                        nx,
                        event.position.y,
                        nz,
                        i32::from(neighbor_state),
                    );
                    break;
                }
            }
        });

        // Trigger block-entity destroy + revert double partner on chest break.
        // Drop logic moved to the BlockEntityDestroyedEvent handler so the
        // destroy → drops chain runs through the event pipeline.
        registrar.on::<BlockBrokenEvent>(Stage::Post, -1, |event, ctx| {
            let state = event.block_state;
            if !block::is_chest(state) {
                return;
            }

            // Queue the destroy — the server will fire
            // BlockEntityDestroyedEvent with `last_state` once it
            // processes the response, and the handler below picks up
            // the drops.
            ctx.world_ctx().destroy_block_entity(
                event.position.x,
                event.position.y,
                event.position.z,
            );

            // Revert double-chest partner to single
            if block::chest_type(state) != 0 {
                let wctx = ctx.world_ctx();
                let facing = block::chest_facing(state);
                let offsets = block::chest_adjacent_offsets(facing);
                for &(dx, dz) in &offsets {
                    let nx = event.position.x + dx;
                    let nz = event.position.z + dz;
                    let neighbor = wctx.get_block(nx, event.position.y, nz);
                    if block::is_chest(neighbor)
                        && block::chest_facing(neighbor) == facing
                        && block::chest_type(neighbor) != 0
                    {
                        let single = block::chest_state(facing, 0);
                        wctx.set_block(nx, event.position.y, nz, single);
                        wctx.mark_chunk_dirty(nx >> 4, nz >> 4);
                        ctx.entities().broadcast_block_change(
                            nx,
                            event.position.y,
                            nz,
                            i32::from(single),
                        );
                        break;
                    }
                }
            }
        });

        // Chest lid open animation when a player opens a chest.
        registrar.on::<ContainerOpenedEvent>(Stage::Post, 0, |event, ctx| {
            let position = match event.backing {
                ContainerBacking::Block { position } => position,
                ContainerBacking::Virtual => return,
            };
            let wctx = ctx.world_ctx();
            if !block::is_chest(wctx.get_block(position.x, position.y, position.z)) {
                return;
            }

            let action_param = event.viewer_count.min(u32::from(u8::MAX)) as u8;
            for (px, py, pz) in chest_parts(wctx, position.x, position.y, position.z) {
                ctx.entities().broadcast_block_action(
                    px,
                    py,
                    pz,
                    1, // chest "viewer count" action
                    action_param,
                    CHEST_BLOCK_ID,
                );
            }
        });

        // Chest lid close animation — `viewer_count` already excludes the
        // closing player, so it's the *remaining* viewer count (0 closes
        // the lid completely).
        registrar.on::<ContainerClosedEvent>(Stage::Post, 0, |event, ctx| {
            let position = match event.backing {
                ContainerBacking::Block { position } => position,
                ContainerBacking::Virtual => return,
            };
            let wctx = ctx.world_ctx();
            if !block::is_chest(wctx.get_block(position.x, position.y, position.z)) {
                return;
            }

            let action_param = event.viewer_count.min(u32::from(u8::MAX)) as u8;
            for (px, py, pz) in chest_parts(wctx, position.x, position.y, position.z) {
                ctx.entities()
                    .broadcast_block_action(px, py, pz, 1, action_param, CHEST_BLOCK_ID);
            }
        });

        // Sync slot changes to other viewers of the same block-backed
        // container so two players staring at the same chest see the
        // same contents.
        registrar.on::<ContainerSlotChangedEvent>(Stage::Post, 0, |event, ctx| {
            let position = match event.backing {
                ContainerBacking::Block { position } => position,
                ContainerBacking::Virtual => return,
            };
            ctx.containers().notify_viewers(
                position.x,
                position.y,
                position.z,
                event.slot_index,
                event.new.clone(),
            );
        });

        // Drop chest contents as item entities when the block entity
        // is destroyed (e.g. by the BlockBrokenEvent handler above).
        registrar.on::<BlockEntityDestroyedEvent>(Stage::Post, 0, |event, ctx| {
            if event.kind != BlockEntityKind::Chest {
                return;
            }
            let BlockEntity::Chest { slots } = &event.last_state;
            for slot in slots.iter() {
                if let Some(item_id) = slot.item_id {
                    ctx.entities().spawn_dropped_item(
                        event.position.x,
                        event.position.y,
                        event.position.z,
                        item_id,
                        slot.item_count,
                    );
                }
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
    fn interact_on_chest_cancels_event() {
        let mut harness = PluginTestHarness::new();
        harness.register(ContainerPlugin);

        let mut event = PlayerInteractEvent {
            position: BlockPosition { x: 5, y: 64, z: 3 },
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
            position: BlockPosition { x: 5, y: 64, z: 3 },
            block_state: block::STONE,
            direction: 1,
            sequence: 1,
            cancelled: false,
        };

        harness.dispatch(&mut event);
        assert!(!event.is_cancelled(), "interact on stone should not cancel");
    }

    #[test]
    fn block_entity_destroyed_drops_chest_contents() {
        let mut harness = PluginTestHarness::new();
        harness.register(ContainerPlugin);

        let mut chest = BlockEntity::empty_chest();
        let BlockEntity::Chest { slots } = &mut chest;
        slots[0] = basalt_api::types::Slot::new(1, 5);
        slots[1] = basalt_api::types::Slot::new(2, 3);

        let mut event = BlockEntityDestroyedEvent {
            position: BlockPosition { x: 5, y: 64, z: 3 },
            kind: BlockEntityKind::Chest,
            last_state: chest,
        };

        let result = harness.dispatch(&mut event);
        assert!(result.has_spawn_dropped_item(1, 5));
        assert!(result.has_spawn_dropped_item(2, 3));
    }

    #[test]
    fn slot_changed_on_block_backed_notifies_viewers() {
        let mut harness = PluginTestHarness::new();
        harness.register(ContainerPlugin);

        let mut event = ContainerSlotChangedEvent {
            window_id: 1,
            backing: ContainerBacking::Block {
                position: BlockPosition { x: 5, y: 64, z: 3 },
            },
            slot_index: 4,
            old: basalt_api::types::Slot::empty(),
            new: basalt_api::types::Slot::new(1, 1),
        };

        let result = harness.dispatch(&mut event);
        assert!(
            result.has_notify_viewers(),
            "block-backed slot change must notify viewers"
        );
    }

    #[test]
    fn slot_changed_on_virtual_does_not_notify() {
        let mut harness = PluginTestHarness::new();
        harness.register(ContainerPlugin);

        let mut event = ContainerSlotChangedEvent {
            window_id: 1,
            backing: ContainerBacking::Virtual,
            slot_index: 4,
            old: basalt_api::types::Slot::empty(),
            new: basalt_api::types::Slot::new(1, 1),
        };

        let result = harness.dispatch(&mut event);
        assert!(
            !result.has_notify_viewers(),
            "virtual slot change must not notify viewers"
        );
    }

    #[test]
    fn chest_open_broadcasts_lid_animation() {
        let mut harness = PluginTestHarness::new();
        harness.world().set_block(5, 64, 3, block::CHEST);
        harness.register(ContainerPlugin);

        let mut event = ContainerOpenedEvent {
            window_id: 1,
            inventory_type: InventoryType::Generic9x3,
            backing: ContainerBacking::Block {
                position: BlockPosition { x: 5, y: 64, z: 3 },
            },
            viewer_count: 1,
        };

        let result = harness.dispatch(&mut event);
        assert!(
            result.has_broadcast_block_action(),
            "chest open must broadcast a BlockAction packet"
        );
    }

    #[test]
    fn virtual_open_broadcasts_no_animation() {
        let mut harness = PluginTestHarness::new();
        harness.register(ContainerPlugin);

        let mut event = ContainerOpenedEvent {
            window_id: 1,
            inventory_type: InventoryType::Generic9x3,
            backing: ContainerBacking::Virtual,
            viewer_count: 0,
        };

        let result = harness.dispatch(&mut event);
        assert!(
            !result.has_broadcast_block_action(),
            "virtual container must not broadcast lid animation"
        );
    }

    #[test]
    fn chest_close_broadcasts_lid_animation() {
        let mut harness = PluginTestHarness::new();
        harness.world().set_block(5, 64, 3, block::CHEST);
        harness.register(ContainerPlugin);

        let mut event = ContainerClosedEvent {
            window_id: 1,
            inventory_type: InventoryType::Generic9x3,
            backing: ContainerBacking::Block {
                position: BlockPosition { x: 5, y: 64, z: 3 },
            },
            reason: CloseReason::Manual,
            viewer_count: 0,
            crafting_grid_state: None,
        };

        let result = harness.dispatch(&mut event);
        assert!(
            result.has_broadcast_block_action(),
            "chest close must broadcast a BlockAction packet (lid close)"
        );
    }

    #[test]
    fn block_broken_chest_queues_destroy() {
        let mut harness = PluginTestHarness::new();
        harness.register(ContainerPlugin);

        let mut event = BlockBrokenEvent {
            position: BlockPosition { x: 5, y: 64, z: 3 },
            block_state: block::CHEST,
            sequence: 1,
            cancelled: false,
        };

        let result = harness.dispatch(&mut event);
        assert!(
            result.has_destroy_block_entity(),
            "breaking a chest must queue a destroy for the block entity"
        );
    }

    #[test]
    fn block_broken_non_chest_does_not_queue_destroy() {
        let mut harness = PluginTestHarness::new();
        harness.register(ContainerPlugin);

        let mut event = BlockBrokenEvent {
            position: BlockPosition { x: 5, y: 64, z: 3 },
            block_state: block::STONE,
            sequence: 1,
            cancelled: false,
        };

        let result = harness.dispatch(&mut event);
        assert!(
            !result.has_destroy_block_entity(),
            "breaking stone must not touch block entities"
        );
    }
}
