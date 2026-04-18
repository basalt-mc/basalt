//! Inventory interactions — window clicks and item drops.

use basalt_types::Uuid;

use super::GameLoop;
use crate::messages::ServerOutput;

impl GameLoop {
    /// Handles dropping the held item via BlockDig status 3/4 (Q key).
    ///
    /// Status 3 = drop entire stack, status 4 = drop single item.
    pub(super) fn handle_item_drop(&mut self, uuid: Uuid, drop_stack: bool) {
        let Some(eid) = self.find_by_uuid(uuid) else {
            return;
        };
        let (item_id, drop_count, held_idx) = {
            let Some(inv) = self.ecs.get::<basalt_core::Inventory>(eid) else {
                return;
            };
            let held_idx = inv.held_slot as usize;
            let item = &inv.slots[held_idx];
            let Some(item_id) = item.item_id else {
                return;
            };
            let count = if drop_stack { item.item_count } else { 1 };
            (item_id, count, held_idx)
        };

        // Decrement or clear the slot
        if let Some(inv) = self.ecs.get_mut::<basalt_core::Inventory>(eid) {
            if drop_count >= inv.slots[held_idx].item_count {
                inv.slots[held_idx] = basalt_types::Slot::empty();
            } else {
                inv.slots[held_idx].item_count -= drop_count;
            }
        }

        // Spawn the dropped item entity
        if let Some(pos) = self.ecs.get::<basalt_core::Position>(eid) {
            self.spawn_item_entity(
                pos.x as i32,
                pos.y as i32 + 1,
                pos.z as i32,
                item_id,
                drop_count,
            );
        }

        // Sync the changed slot (raw internal index = SetPlayerInventory slot)
        let slot_after = self
            .ecs
            .get::<basalt_core::Inventory>(eid)
            .map(|inv| inv.slots[held_idx].clone())
            .unwrap_or_default();
        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::SetSlot {
                slot: held_idx as i16,
                item: slot_after,
            });
        });
    }

    /// Handles a player inventory click.
    ///
    /// The client sends the expected result in `changed_slots` and
    /// `cursor_item`. We apply them server-side and handle drops.
    ///
    /// Key flows:
    /// - Click outside (slot -999): drop the OLD cursor item
    /// - Mode 4 (Q key in inventory): drop from hovered slot
    /// - All others: apply changed_slots + update cursor
    pub(super) fn handle_window_click(
        &mut self,
        uuid: Uuid,
        slot: i16,
        button: i8,
        mode: i32,
        changed_slots: Vec<(i16, basalt_types::Slot)>,
        cursor_item: basalt_types::Slot,
    ) {
        let Some(eid) = self.find_by_uuid(uuid) else {
            return;
        };

        // If a container is open, route to container click handler
        if let Some(oc) = self.ecs.get::<basalt_core::OpenContainer>(eid) {
            let pos = oc.position;
            // Drop outside container window
            if slot == -999 {
                let old_cursor = self
                    .ecs
                    .get::<basalt_core::Inventory>(eid)
                    .map(|inv| inv.cursor.clone())
                    .unwrap_or_default();
                if let Some(item_id) = old_cursor.item_id
                    && let Some(player_pos) = self.ecs.get::<basalt_core::Position>(eid)
                {
                    self.spawn_item_entity(
                        player_pos.x as i32,
                        player_pos.y as i32 + 1,
                        player_pos.z as i32,
                        item_id,
                        old_cursor.item_count,
                    );
                }
                if let Some(inv) = self.ecs.get_mut::<basalt_core::Inventory>(eid) {
                    inv.cursor = cursor_item;
                }
                return;
            }
            // Mode 4: Q key drop while hovering a container slot
            if mode == 4 && slot >= 0 {
                // Determine what to drop: container slot or player slot
                let ws = slot;
                let drop_item = if (0..27).contains(&ws) {
                    // Chest slot
                    self.world
                        .get_block_entity(pos.0, pos.1, pos.2)
                        .map(|be| match &*be {
                            basalt_world::block_entity::BlockEntity::Chest { slots } => {
                                slots[ws as usize].clone()
                            }
                        })
                } else if (27..54).contains(&ws) {
                    let idx = (ws - 27 + 9) as usize;
                    self.ecs
                        .get::<basalt_core::Inventory>(eid)
                        .and_then(|inv| (idx < 36).then(|| inv.slots[idx].clone()))
                } else if (54..63).contains(&ws) {
                    let idx = (ws - 54) as usize;
                    self.ecs
                        .get::<basalt_core::Inventory>(eid)
                        .map(|inv| inv.slots[idx].clone())
                } else {
                    None
                };

                if let Some(item) = drop_item
                    && let Some(item_id) = item.item_id
                {
                    let drop_count = if button == 0 { 1 } else { item.item_count };
                    // Apply the changed_slots from the client (handles decrement)
                    self.handle_container_click(eid, pos, &changed_slots, cursor_item);
                    // Spawn the dropped item
                    if let Some(player_pos) = self.ecs.get::<basalt_core::Position>(eid) {
                        self.spawn_item_entity(
                            player_pos.x as i32,
                            player_pos.y as i32 + 1,
                            player_pos.z as i32,
                            item_id,
                            drop_count,
                        );
                    }
                }
                return;
            }

            self.handle_container_click(eid, pos, &changed_slots, cursor_item);
            return;
        }

        // Click outside window (slot -999): drop what was on the cursor
        if slot == -999 {
            let old_cursor = {
                let Some(inv) = self.ecs.get::<basalt_core::Inventory>(eid) else {
                    return;
                };
                inv.cursor.clone()
            };
            if let Some(item_id) = old_cursor.item_id
                && let Some(pos) = self.ecs.get::<basalt_core::Position>(eid)
            {
                self.spawn_item_entity(
                    pos.x as i32,
                    pos.y as i32 + 1,
                    pos.z as i32,
                    item_id,
                    old_cursor.item_count,
                );
            }
            // Update cursor (now empty) and apply any changed_slots
            if let Some(inv) = self.ecs.get_mut::<basalt_core::Inventory>(eid) {
                inv.cursor = cursor_item;
                for (window_slot, item) in &changed_slots {
                    if let Some(idx) = basalt_core::Inventory::window_to_index(*window_slot) {
                        inv.slots[idx] = item.clone();
                    }
                }
            }
            return;
        }

        // Mode 4: Q key while hovering a slot in open inventory
        if mode == 4 && slot >= 0 {
            if let Some(idx) = basalt_core::Inventory::window_to_index(slot) {
                let item = {
                    let Some(inv) = self.ecs.get::<basalt_core::Inventory>(eid) else {
                        return;
                    };
                    inv.slots[idx].clone()
                };
                if let Some(item_id) = item.item_id {
                    let drop_count = if button == 0 { 1 } else { item.item_count };
                    if let Some(inv) = self.ecs.get_mut::<basalt_core::Inventory>(eid) {
                        if drop_count >= inv.slots[idx].item_count {
                            inv.slots[idx] = basalt_types::Slot::empty();
                        } else {
                            inv.slots[idx].item_count -= drop_count;
                        }
                    }
                    if let Some(pos) = self.ecs.get::<basalt_core::Position>(eid) {
                        self.spawn_item_entity(
                            pos.x as i32,
                            pos.y as i32 + 1,
                            pos.z as i32,
                            item_id,
                            drop_count,
                        );
                    }
                    let slot_after = self
                        .ecs
                        .get::<basalt_core::Inventory>(eid)
                        .map(|inv| inv.slots[idx].clone())
                        .unwrap_or_default();
                    self.send_to(eid, |tx| {
                        let _ = tx.try_send(ServerOutput::SetSlot {
                            slot: idx as i16,
                            item: slot_after,
                        });
                    });
                }
            }
            return;
        }

        // All other clicks: apply changed_slots + update cursor
        if let Some(inv) = self.ecs.get_mut::<basalt_core::Inventory>(eid) {
            for (window_slot, item) in &changed_slots {
                if let Some(idx) = basalt_core::Inventory::window_to_index(*window_slot) {
                    inv.slots[idx] = item.clone();
                }
            }
            inv.cursor = cursor_item;
        }
    }
}

#[cfg(test)]
mod tests {
    use basalt_types::Uuid;

    use crate::messages::{GameInput, ServerOutput};

    #[test]
    fn held_item_slot_change() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let _ = game_tx.send(GameInput::HeldItemSlot { uuid, slot: 3 });
        game_loop.tick(1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert_eq!(inv.held_slot, 3);
    }

    #[test]
    fn set_creative_slot() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let _ = game_tx.send(GameInput::SetCreativeSlot {
            uuid,
            slot: 36,
            item: basalt_types::Slot {
                item_id: Some(1),
                item_count: 64,
                component_data: vec![],
            },
        });
        game_loop.tick(1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert_eq!(inv.hotbar()[0].item_id, Some(1));
        assert_eq!(inv.hotbar()[0].item_count, 64);
    }

    #[test]
    fn set_creative_slot_out_of_range_ignored() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let _ = game_tx.send(GameInput::SetCreativeSlot {
            uuid,
            slot: 10,
            item: basalt_types::Slot {
                item_id: Some(1),
                item_count: 1,
                component_data: vec![],
            },
        });
        game_loop.tick(1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert!(inv.hotbar()[0].item_id.is_none());
    }

    #[test]
    fn window_click_outside_drops_cursor() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        // Set cursor item directly
        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .cursor = basalt_types::Slot::new(1, 16);

        while rx.try_recv().is_ok() {}

        // Click outside window (slot -999)
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: -999,
            button: 0,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        // Cursor should be empty
        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert!(
            inv.cursor.is_empty(),
            "cursor should be empty after drop outside"
        );

        // Should spawn item entity
        let mut got_spawn = false;
        while let Ok(msg) = rx.try_recv() {
            if matches!(&msg, ServerOutput::Broadcast(_)) {
                got_spawn = true;
            }
        }
        assert!(got_spawn, "should spawn dropped item from cursor");
    }

    #[test]
    fn window_click_applies_changed_slots() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .slots[0] = basalt_types::Slot::new(1, 10);

        // Swap hotbar slot 0 to main slot 9 (window: 36 → 9)
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 36,
            button: 0,
            mode: 0,
            changed_slots: vec![
                (36, basalt_types::Slot::empty()),
                (9, basalt_types::Slot::new(1, 10)),
            ],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert!(inv.slots[0].is_empty(), "hotbar 0 should be empty");
        assert_eq!(inv.slots[9].item_id, Some(1), "main 0 should have item");
        assert_eq!(inv.slots[9].item_count, 10);
    }

    #[test]
    fn q_key_drop_single_item() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        // Give 10 stone in hotbar slot 0
        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .slots[0] = basalt_types::Slot::new(1, 10);

        while rx.try_recv().is_ok() {}

        // Q key = BlockDig status 4 (drop single)
        let _ = game_tx.send(GameInput::BlockDig {
            uuid,
            status: 4,
            x: 0,
            y: 0,
            z: 0,
            sequence: 0,
        });
        game_loop.tick(1);

        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert_eq!(inv.slots[0].item_count, 9, "should have 9 after dropping 1");

        // Should receive SetSlot + SpawnEntity broadcast
        let mut got_set_slot = false;
        let mut got_spawn = false;
        while let Ok(msg) = rx.try_recv() {
            match &msg {
                ServerOutput::SetSlot { slot, item } => {
                    assert_eq!(*slot, 0);
                    assert_eq!(item.item_count, 9);
                    got_set_slot = true;
                }
                ServerOutput::Broadcast(_) => got_spawn = true,
                _ => {}
            }
        }
        assert!(got_set_slot, "should sync hotbar slot");
        assert!(got_spawn, "should spawn dropped item entity");
    }

    #[test]
    fn ctrl_q_drop_full_stack() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .slots[0] = basalt_types::Slot::new(1, 32);

        while rx.try_recv().is_ok() {}

        // Ctrl+Q = BlockDig status 3 (drop stack)
        let _ = game_tx.send(GameInput::BlockDig {
            uuid,
            status: 3,
            x: 0,
            y: 0,
            z: 0,
            sequence: 0,
        });
        game_loop.tick(1);

        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert!(
            inv.slots[0].is_empty(),
            "slot should be empty after full drop"
        );
    }

    #[test]
    fn creative_drop_slot_minus_one() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        // Creative drop: SetCreativeSlot with slot -1
        let _ = game_tx.send(GameInput::SetCreativeSlot {
            uuid,
            slot: -1,
            item: basalt_types::Slot::new(1, 5),
        });
        game_loop.tick(1);

        // Should spawn a dropped item entity
        let mut got_spawn = false;
        while let Ok(msg) = rx.try_recv() {
            if matches!(&msg, ServerOutput::Broadcast(_)) {
                got_spawn = true;
            }
        }
        assert!(got_spawn, "creative drop should spawn item entity");
    }
}
