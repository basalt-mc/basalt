//! Block interactions — dig (break) and place handling.

use std::sync::Arc;

use basalt_api::context::ServerContext;
use basalt_api::events::{BlockBrokenEvent, BlockPlacedEvent, PlayerInteractEvent};
use basalt_events::Event;
use basalt_types::Uuid;

use super::{GameLoop, OutputHandle, Sneaking};
use crate::messages::ServerOutput;

impl GameLoop {
    /// Handles a block dig (break).
    pub(super) fn handle_block_dig(&mut self, uuid: Uuid, x: i32, y: i32, z: i32, sequence: i32) {
        let Some(eid) = self.find_by_uuid(uuid) else {
            return;
        };
        let (entity_id, username) = {
            let Some(pr) = self.ecs.get::<basalt_core::PlayerRef>(eid) else {
                return;
            };
            (eid as i32, pr.username.clone())
        };

        let original_state = self.world.get_block(x, y, z);
        let ctx = ServerContext::new(Arc::clone(&self.world), uuid, entity_id, username, 0.0, 0.0);
        let mut event = BlockBrokenEvent {
            position: basalt_core::BlockPosition { x, y, z },
            block_state: original_state,
            sequence,
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
        let Some(eid) = self.find_by_uuid(uuid) else {
            return;
        };

        // Dispatch PlayerInteractEvent for the clicked block.
        // Plugins (e.g., ContainerPlugin) can cancel to prevent placement.
        let is_sneaking = self.ecs.has::<Sneaking>(eid);
        let clicked_state = self.world.get_block(x, y, z);
        if !is_sneaking {
            let entity_id = eid as i32;
            let username = self
                .ecs
                .get::<basalt_core::PlayerRef>(eid)
                .map_or_else(String::new, |pr| pr.username.clone());
            let (yaw, pitch) = self
                .ecs
                .get::<basalt_core::Rotation>(eid)
                .map_or((0.0, 0.0), |r| (r.yaw, r.pitch));
            let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);
            let mut interact = PlayerInteractEvent {
                position: basalt_core::BlockPosition { x, y, z },
                block_state: clicked_state,
                direction,
                sequence,
                cancelled: false,
            };
            self.bus.dispatch(&mut interact, &ctx);
            self.process_responses(uuid, &ctx.drain_responses());
            if interact.is_cancelled() {
                return;
            }
        }

        let (dx, dy, dz) = face_offset(direction);
        let (px, py, pz) = (x + dx, y + dy, z + dz);

        let (entity_id, username, block_state) = {
            let Some(inv) = self.ecs.get::<basalt_core::Inventory>(eid) else {
                return;
            };
            let Some(item_id) = inv.held_item().item_id else {
                return;
            };
            let Some(block_state) = basalt_world::block::item_to_default_block_state(item_id)
            else {
                return;
            };
            let Some(pr) = self.ecs.get::<basalt_core::PlayerRef>(eid) else {
                return;
            };
            (eid as i32, pr.username.clone(), block_state)
        };

        let (yaw, pitch) = self
            .ecs
            .get::<basalt_core::Rotation>(eid)
            .map_or((0.0, 0.0), |r| (r.yaw, r.pitch));
        let ctx = ServerContext::new(
            Arc::clone(&self.world),
            uuid,
            entity_id,
            username,
            yaw,
            pitch,
        );
        let mut event = BlockPlacedEvent {
            position: basalt_core::BlockPosition {
                x: px,
                y: py,
                z: pz,
            },
            block_state,
            sequence,
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
        // Block entity creation, chest orientation, and double chest
        // pairing are handled by ContainerPlugin via BlockPlacedEvent Post.
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

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        if let Some(inv) = game_loop.ecs.get_mut::<basalt_core::Inventory>(eid) {
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
        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
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

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        // Set player yaw to 180 (facing north → chest faces south)
        game_loop
            .ecs
            .get_mut::<basalt_core::Rotation>(eid)
            .unwrap()
            .yaw = 180.0;
        // Give chest in hotbar
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
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

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_core::Rotation>(eid)
            .unwrap()
            .yaw = 0.0; // facing south → chest faces north

        // Place first chest
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
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
