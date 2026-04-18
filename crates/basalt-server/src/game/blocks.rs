//! Block interactions — dig (break) and place handling.

use std::sync::Arc;

use basalt_api::context::ServerContext;
use basalt_api::events::{BlockBrokenEvent, BlockPlacedEvent};
use basalt_events::Event;
use basalt_types::Uuid;

use super::{GameLoop, OutputHandle, Sneaking};
use crate::messages::{BroadcastEvent, ServerOutput, SharedBroadcast};

impl GameLoop {
    /// Handles a block dig (break).
    pub(super) fn handle_block_dig(&mut self, uuid: Uuid, x: i32, y: i32, z: i32, sequence: i32) {
        let Some(eid) = self.ecs.find_by_uuid(uuid) else {
            return;
        };
        let (entity_id, username) = {
            let Some(pr) = self.ecs.get::<basalt_ecs::PlayerRef>(eid) else {
                return;
            };
            (eid as i32, pr.username.clone())
        };

        let original_state = self.world.get_block(x, y, z);
        let ctx = ServerContext::new(Arc::clone(&self.world), uuid, entity_id, username, 0.0, 0.0);
        let mut event = BlockBrokenEvent {
            x,
            y,
            z,
            block_state: original_state,
            sequence,
            player_uuid: uuid,
            cancelled: false,
        };
        self.bus.dispatch(&mut event, &ctx);

        if event.is_cancelled() {
            if let Some(handle) = self.ecs.get::<OutputHandle>(eid) {
                let _ = handle.tx.try_send(ServerOutput::BlockChanged {
                    x,
                    y,
                    z,
                    state: i32::from(original_state),
                });
            }
            return;
        }

        self.process_responses(uuid, &ctx.drain_responses());

        // Collect items to drop from block entity before removing it
        let items_to_drop: Vec<(i32, i32)> = self
            .world
            .get_block_entity(x, y, z)
            .map(|be| match &*be {
                basalt_world::block_entity::BlockEntity::Chest { slots } => slots
                    .iter()
                    .filter_map(|s| s.item_id.map(|id| (id, s.item_count)))
                    .collect(),
            })
            .unwrap_or_default();

        self.world.remove_block_entity(x, y, z);

        // Spawn dropped items for chest contents
        for (item_id, count) in items_to_drop {
            self.spawn_item_entity(x, y, z, item_id, count);
        }

        // If this was part of a double chest, revert the other half to single
        if basalt_world::block::is_chest(original_state)
            && basalt_world::block::chest_type(original_state) != 0
        {
            let facing = basalt_world::block::chest_facing(original_state);
            let offsets = basalt_world::block::chest_adjacent_offsets(facing);
            for (dx, dz) in offsets {
                let nx = x + dx;
                let nz = z + dz;
                let neighbor = self.world.get_block(nx, y, nz);
                if basalt_world::block::is_chest(neighbor)
                    && basalt_world::block::chest_facing(neighbor) == facing
                    && basalt_world::block::chest_type(neighbor) != 0
                {
                    let single = basalt_world::block::chest_state(facing, 0);
                    self.world.set_block(nx, y, nz, single);
                    self.chunk_cache.invalidate(nx >> 4, nz >> 4);
                    let bc = Arc::new(SharedBroadcast::new(BroadcastEvent::BlockChanged {
                        x: nx,
                        y,
                        z: nz,
                        state: i32::from(single),
                    }));
                    for (e, _) in self.ecs.iter::<OutputHandle>() {
                        self.send_to(e, |tx| {
                            let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&bc)));
                        });
                    }
                    break;
                }
            }
        }
    }

    /// Handles a block place.
    pub(super) fn handle_block_place(
        &mut self,
        uuid: Uuid,
        x: i32,
        y: i32,
        z: i32,
        direction: i32,
        sequence: i32,
    ) {
        let Some(eid) = self.ecs.find_by_uuid(uuid) else {
            return;
        };

        // Check if the clicked block is an interactable container
        // Sneaking players skip interaction and place blocks instead
        let is_sneaking = self.ecs.has::<Sneaking>(eid);
        let clicked_state = self.world.get_block(x, y, z);
        if !is_sneaking && basalt_world::block::is_chest(clicked_state) {
            self.open_chest(eid, x, y, z);
            return;
        }

        let (dx, dy, dz) = face_offset(direction);
        let (px, py, pz) = (x + dx, y + dy, z + dz);

        let (entity_id, username, block_state) = {
            let Some(inv) = self.ecs.get::<basalt_ecs::Inventory>(eid) else {
                return;
            };
            let Some(item_id) = inv.held_item().item_id else {
                return;
            };
            let Some(mut block_state) = basalt_world::block::item_to_default_block_state(item_id)
            else {
                return;
            };
            // Orient directional blocks based on player yaw
            if basalt_world::block::is_chest(block_state) {
                let yaw = self
                    .ecs
                    .get::<basalt_ecs::Rotation>(eid)
                    .map_or(0.0, |r| r.yaw);
                block_state = basalt_world::block::chest_state_for_yaw(yaw);
            }
            let Some(pr) = self.ecs.get::<basalt_ecs::PlayerRef>(eid) else {
                return;
            };
            (eid as i32, pr.username.clone(), block_state)
        };

        let ctx = ServerContext::new(Arc::clone(&self.world), uuid, entity_id, username, 0.0, 0.0);
        let mut event = BlockPlacedEvent {
            x: px,
            y: py,
            z: pz,
            block_state,
            sequence,
            player_uuid: uuid,
            cancelled: false,
        };
        self.bus.dispatch(&mut event, &ctx);

        if event.is_cancelled() {
            if let Some(handle) = self.ecs.get::<OutputHandle>(eid) {
                let _ = handle.tx.try_send(ServerOutput::BlockChanged {
                    x: px,
                    y: py,
                    z: pz,
                    state: i32::from(basalt_world::block::AIR),
                });
            }
            return;
        }

        self.process_responses(uuid, &ctx.drain_responses());

        // Create block entity for interactive blocks (chests)
        if basalt_world::block::is_chest(block_state) {
            self.world.set_block_entity(
                px,
                py,
                pz,
                basalt_world::block_entity::BlockEntity::empty_chest(),
            );

            // Double chest pairing logic:
            // - Not sneaking: scan adjacent blocks for a single chest to pair with
            // - Sneaking + clicked a chest: pair only with the clicked chest
            // - Sneaking + clicked non-chest: no pairing (single chest)
            let facing = basalt_world::block::chest_facing(block_state);
            let mut paired = false;

            // Build candidate list: either all adjacent or just the clicked chest
            let candidates: Vec<(i32, i32)> = if !is_sneaking {
                basalt_world::block::chest_adjacent_offsets(facing)
                    .iter()
                    .map(|&(ddx, ddz)| (px + ddx, pz + ddz))
                    .collect()
            } else if basalt_world::block::is_chest(clicked_state) {
                // Sneaking on a chest: pair only if new chest is lateral (left/right)
                let valid_offsets = basalt_world::block::chest_adjacent_offsets(facing);
                let actual_offset = (px - x, pz - z);
                if valid_offsets.contains(&actual_offset) {
                    vec![(x, z)]
                } else {
                    vec![] // front/back placement: no pairing
                }
            } else {
                vec![] // sneaking on non-chest: no pairing
            };

            for &(nx, nz) in &candidates {
                let neighbor = self.world.get_block(nx, py, nz);
                if basalt_world::block::is_single_chest(neighbor)
                    && basalt_world::block::chest_facing(neighbor) == facing
                {
                    // Compute offset from new chest to neighbor
                    let ddx = nx - px;
                    let ddz = nz - pz;
                    let (new_type, existing_type) =
                        basalt_world::block::chest_double_types(facing, ddx, ddz);
                    let new_state = basalt_world::block::chest_state(facing, new_type);
                    self.world.set_block(px, py, pz, new_state);
                    let neighbor_state = basalt_world::block::chest_state(facing, existing_type);
                    self.world.set_block(nx, py, nz, neighbor_state);
                    self.chunk_cache.invalidate(px >> 4, pz >> 4);
                    self.chunk_cache.invalidate(nx >> 4, nz >> 4);
                    for (e, _) in self.ecs.iter::<OutputHandle>() {
                        self.send_to(e, |tx| {
                            let _ = tx.try_send(ServerOutput::BlockChanged {
                                x: px,
                                y: py,
                                z: pz,
                                state: i32::from(new_state),
                            });
                            let _ = tx.try_send(ServerOutput::BlockEntityData {
                                x: px,
                                y: py,
                                z: pz,
                                action: 2,
                            });
                            let _ = tx.try_send(ServerOutput::BlockChanged {
                                x: nx,
                                y: py,
                                z: nz,
                                state: i32::from(neighbor_state),
                            });
                            let _ = tx.try_send(ServerOutput::BlockEntityData {
                                x: nx,
                                y: py,
                                z: nz,
                                action: 2,
                            });
                        });
                    }
                    paired = true;
                    break;
                }
            }

            if !paired {
                // Single chest — broadcast normally
                for (e, _) in self.ecs.iter::<OutputHandle>() {
                    self.send_to(e, |tx| {
                        let _ = tx.try_send(ServerOutput::BlockChanged {
                            x: px,
                            y: py,
                            z: pz,
                            state: i32::from(block_state),
                        });
                        let _ = tx.try_send(ServerOutput::BlockEntityData {
                            x: px,
                            y: py,
                            z: pz,
                            action: 2,
                        });
                    });
                }
            }
        }
    }
}

/// Returns the (dx, dy, dz) offset for a block face direction.
pub(super) fn face_offset(direction: i32) -> (i32, i32, i32) {
    match direction {
        0 => (0, -1, 0),
        1 => (0, 1, 0),
        2 => (0, 0, -1),
        3 => (0, 0, 1),
        4 => (-1, 0, 0),
        5 => (1, 0, 0),
        _ => (0, 0, 0),
    }
}

#[cfg(test)]
mod tests {
    use basalt_types::Uuid;

    use crate::messages::{BroadcastEvent, GameInput, ServerOutput};

    #[test]
    fn block_dig_sets_air() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::STONE);

        let _ = game_tx.send(GameInput::BlockDig {
            uuid,
            status: 0,
            x: 5,
            y: 64,
            z: 3,
            sequence: 42,
        });
        game_loop.tick(2);
        assert_eq!(
            game_loop.world.get_block(5, 64, 3),
            basalt_world::block::AIR
        );
    }

    #[test]
    fn block_dig_sends_ack_and_broadcast() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::STONE);

        while rx.try_recv().is_ok() {}

        let _ = game_tx.send(GameInput::BlockDig {
            uuid,
            status: 0,
            x: 5,
            y: 64,
            z: 3,
            sequence: 42,
        });
        game_loop.tick(2);

        let mut got_ack = false;
        let mut got_block_change = false;
        while let Ok(msg) = rx.try_recv() {
            match &msg {
                ServerOutput::BlockAck { .. } => got_ack = true,
                ServerOutput::Broadcast(bc) => {
                    if matches!(bc.event, BroadcastEvent::BlockChanged { .. }) {
                        got_block_change = true;
                    }
                }
                _ => {}
            }
        }
        assert!(got_ack, "should have received block ack");
        assert!(
            got_block_change,
            "should have received block change broadcast"
        );
    }

    #[test]
    fn block_dig_for_unknown_player_ignored() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let unknown = Uuid::from_bytes([99; 16]);

        let _ = game_tx.send(GameInput::BlockDig {
            uuid: unknown,
            status: 0,
            x: 5,
            y: 64,
            z: 3,
            sequence: 1,
        });
        game_loop.tick(0);
    }

    #[test]
    fn block_place_with_held_item() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        if let Some(inv) = game_loop.ecs.get_mut::<basalt_ecs::Inventory>(eid) {
            inv.hotbar_mut()[0] = basalt_types::Slot {
                item_id: Some(1),
                item_count: 1,
                component_data: vec![],
            };
        }

        let _ = game_tx.send(GameInput::BlockPlace {
            uuid,
            x: 5,
            y: 63,
            z: 3,
            direction: 1,
            sequence: 10,
        });
        game_loop.tick(2);

        assert_eq!(
            game_loop.world.get_block(5, 64, 3),
            basalt_world::block::STONE
        );
    }

    #[test]
    fn chest_placement_creates_block_entity() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        // Give chest in hotbar slot 0 (item 280 = chest)
        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_ecs::Inventory>(eid)
            .unwrap()
            .slots[0] = basalt_types::Slot::new(313, 1); // chest item ID

        // Place chest on top of (5, -60, 3)
        let _ = game_tx.send(GameInput::BlockPlace {
            uuid,
            x: 5,
            y: -60,
            z: 3,
            direction: 1,
            sequence: 10,
        });
        game_loop.tick(1);

        // Block entity should exist
        assert!(
            game_loop.world.get_block_entity(5, -59, 3).is_some(),
            "chest placement should create block entity"
        );
    }

    #[test]
    fn chest_orientation_based_on_yaw() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        // Set player yaw to 180 (facing north → chest faces south)
        game_loop
            .ecs
            .get_mut::<basalt_ecs::Rotation>(eid)
            .unwrap()
            .yaw = 180.0;
        // Give chest in hotbar
        game_loop
            .ecs
            .get_mut::<basalt_ecs::Inventory>(eid)
            .unwrap()
            .slots[0] = basalt_types::Slot::new(313, 1);

        let _ = game_tx.send(GameInput::BlockPlace {
            uuid,
            x: 5,
            y: -60,
            z: 3,
            direction: 1,
            sequence: 1,
        });
        game_loop.tick(1);

        // Chest at (5, -59, 3) should face south (state 3016)
        let state = game_loop.world.get_block(5, -59, 3);
        assert_eq!(
            state,
            basalt_world::block::chest_state_for_yaw(180.0),
            "chest should face south when player faces north"
        );
    }

    #[test]
    fn double_chest_forms_on_adjacent_placement() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_ecs::Rotation>(eid)
            .unwrap()
            .yaw = 0.0; // facing south → chest faces north

        // Place first chest
        game_loop
            .ecs
            .get_mut::<basalt_ecs::Inventory>(eid)
            .unwrap()
            .slots[0] = basalt_types::Slot::new(313, 2);

        let _ = game_tx.send(GameInput::BlockPlace {
            uuid,
            x: 5,
            y: -60,
            z: 3,
            direction: 1,
            sequence: 1,
        });
        game_loop.tick(1);

        let first_state = game_loop.world.get_block(5, -59, 3);
        assert!(
            basalt_world::block::is_single_chest(first_state),
            "first chest should be single"
        );

        // Place second chest adjacent (east, +X)
        let _ = game_tx.send(GameInput::BlockPlace {
            uuid,
            x: 6,
            y: -60,
            z: 3,
            direction: 1,
            sequence: 2,
        });
        game_loop.tick(2);

        let left = game_loop.world.get_block(5, -59, 3);
        let right = game_loop.world.get_block(6, -59, 3);
        assert!(basalt_world::block::is_chest(left), "left should be chest");
        assert!(
            basalt_world::block::is_chest(right),
            "right should be chest"
        );
        assert_ne!(
            basalt_world::block::chest_type(left),
            0,
            "left should not be single"
        );
        assert_ne!(
            basalt_world::block::chest_type(right),
            0,
            "right should not be single"
        );
        assert_ne!(
            basalt_world::block::chest_type(left),
            basalt_world::block::chest_type(right),
            "left and right should have different types"
        );
    }

    #[test]
    fn breaking_double_chest_reverts_other_to_single() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        // Manually place a double chest (north-facing, left at x=5, right at x=6)
        let left_state = basalt_world::block::chest_state(0, 1); // north, left
        let right_state = basalt_world::block::chest_state(0, 2); // north, right
        game_loop.world.set_block(5, 64, 3, left_state);
        game_loop.world.set_block(6, 64, 3, right_state);
        game_loop.world.set_block_entity(
            5,
            64,
            3,
            basalt_world::block_entity::BlockEntity::empty_chest(),
        );
        game_loop.world.set_block_entity(
            6,
            64,
            3,
            basalt_world::block_entity::BlockEntity::empty_chest(),
        );

        // Break the left half
        let _ = game_tx.send(GameInput::BlockDig {
            uuid,
            status: 0,
            x: 5,
            y: 64,
            z: 3,
            sequence: 1,
        });
        game_loop.tick(1);

        // Right half should be single now
        let remaining = game_loop.world.get_block(6, 64, 3);
        assert!(
            basalt_world::block::is_single_chest(remaining),
            "remaining half should revert to single chest"
        );
    }

    #[test]
    fn chest_break_drops_contents_and_self() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        // Place chest with items inside
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CHEST);
        let mut be = basalt_world::block_entity::BlockEntity::empty_chest();
        let basalt_world::block_entity::BlockEntity::Chest { ref mut slots } = be;
        slots[0] = basalt_types::Slot::new(42, 16);
        game_loop.world.set_block_entity(5, 64, 3, be);

        // Break it
        let _ = game_tx.send(GameInput::BlockDig {
            uuid,
            status: 0,
            x: 5,
            y: 64,
            z: 3,
            sequence: 1,
        });
        game_loop.tick(1);

        // Block entity removed
        assert!(game_loop.world.get_block_entity(5, 64, 3).is_none());

        // Should have spawned dropped items (chest contents + chest block itself)
        let mut spawn_count = 0;
        while let Ok(msg) = rx.try_recv() {
            if matches!(&msg, ServerOutput::Broadcast(bc) if matches!(bc.event, BroadcastEvent::SpawnItemEntity { .. }))
            {
                spawn_count += 1;
            }
        }
        // At least 2 spawns: 1 for the item inside + 1 for the chest block itself
        assert!(
            spawn_count >= 2,
            "should drop chest contents + chest block, got {spawn_count} spawns"
        );
    }

    #[test]
    fn chest_break_removes_block_entity() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        // Place a chest block + entity manually
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CHEST);
        game_loop.world.set_block_entity(
            5,
            64,
            3,
            basalt_world::block_entity::BlockEntity::empty_chest(),
        );

        // Break it
        let _ = game_tx.send(GameInput::BlockDig {
            uuid,
            status: 0,
            x: 5,
            y: 64,
            z: 3,
            sequence: 1,
        });
        game_loop.tick(1);

        assert!(
            game_loop.world.get_block_entity(5, 64, 3).is_none(),
            "breaking chest should remove block entity"
        );
    }
}
