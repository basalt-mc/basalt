//! Drop processors for cursor drops and Q-key slot drops.

use basalt_types::Uuid;

use super::super::slots::WindowType;
use crate::game::GameLoop;
use crate::game::click::WindowSlot;

impl GameLoop {
    /// Drops items from the cursor (click outside window).
    pub(in crate::game::inventory) fn process_drop_cursor(
        &mut self,
        eid: basalt_ecs::EntityId,
        drop_all: bool,
    ) {
        let cursor = self
            .ecs
            .get::<basalt_core::Inventory>(eid)
            .map(|inv| inv.cursor.clone())
            .unwrap_or_default();
        let Some(item_id) = cursor.item_id else {
            return;
        };

        let drop_count = if drop_all { cursor.item_count } else { 1 };

        if let Some(pos) = self.ecs.get::<basalt_core::Position>(eid) {
            self.spawn_item_entity(
                pos.x as i32,
                pos.y as i32 + 1,
                pos.z as i32,
                item_id,
                drop_count,
            );
        }

        if let Some(inv) = self.ecs.get_mut::<basalt_core::Inventory>(eid) {
            if drop_count >= inv.cursor.item_count {
                inv.cursor = basalt_types::Slot::empty();
            } else {
                inv.cursor.item_count -= drop_count;
            }
        }
    }

    /// Drops items from a specific slot (Q key while hovering).
    pub(in crate::game::inventory) fn process_drop_slot(
        &mut self,
        uuid: Uuid,
        eid: basalt_ecs::EntityId,
        ws: &WindowSlot,
        drop_all: bool,
        wt: &WindowType,
        container_pos: Option<(i32, i32, i32)>,
    ) {
        let item = self.read_slot(eid, ws, container_pos);
        let Some(item_id) = item.item_id else { return };

        let drop_count = if drop_all { item.item_count } else { 1 };

        if let Some(pos) = self.ecs.get::<basalt_core::Position>(eid) {
            self.spawn_item_entity(
                pos.x as i32,
                pos.y as i32 + 1,
                pos.z as i32,
                item_id,
                drop_count,
            );
        }

        let new_item = if drop_count >= item.item_count {
            basalt_types::Slot::empty()
        } else {
            basalt_types::Slot::new(item_id, item.item_count - drop_count)
        };
        self.write_slot(eid, ws, new_item.clone(), container_pos);
        self.sync_slot(eid, wt, ws, new_item.clone());

        if let WindowSlot::Container(i) = ws {
            self.dispatch_container_slot_changed(uuid, eid, wt, *i as i16, item, new_item);
        }
    }
}

#[cfg(test)]
mod tests {
    use basalt_types::{Slot, Uuid};

    use crate::game::tests::{connect_player, test_game_loop};
    use crate::messages::{GameInput, ServerOutput};

    /// Sends a WindowClick and ticks the game loop.
    fn click(
        game_tx: &tokio::sync::mpsc::UnboundedSender<GameInput>,
        game_loop: &mut crate::game::GameLoop,
        uuid: Uuid,
        slot: i16,
        button: i8,
        mode: i32,
    ) {
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot,
            button,
            mode,
            changed_slots: vec![],
            cursor_item: Slot::empty(),
        });
        game_loop.tick(1);
    }

    // ── Drop ───────────────────────────────────────────────────

    #[test]
    fn drop_slot_q_key_in_inventory() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .slots[0] = Slot::new(1, 10);
        while rx.try_recv().is_ok() {}

        // Q key on hotbar 0 (mode 4, button 0 = single)
        click(&game_tx, &mut game_loop, uuid, 36, 0, 4);

        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert_eq!(inv.slots[0].item_count, 9);
    }

    #[test]
    fn drop_cursor_outside_window() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .cursor = Slot::new(1, 16);
        while rx.try_recv().is_ok() {}

        // Click outside (slot -999, mode 0, button 0)
        // parse_click_action(-999, 0, 0) => LeftClick{slot: -999}
        // But mode 4, slot -999 => DropCursor
        click(&game_tx, &mut game_loop, uuid, -999, 0, 4);

        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert_eq!(inv.cursor.item_count, 15, "should drop 1 item");
    }

    #[test]
    fn drop_cursor_full_stack() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .cursor = Slot::new(1, 16);
        while rx.try_recv().is_ok() {}

        // Drop all (mode 4, button 1, slot -999)
        click(&game_tx, &mut game_loop, uuid, -999, 1, 4);

        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert!(
            inv.cursor.is_empty(),
            "cursor should be empty after full drop"
        );

        let mut got_spawn = false;
        while let Ok(msg) = rx.try_recv() {
            if matches!(&msg, ServerOutput::Broadcast(_)) {
                got_spawn = true;
            }
        }
        assert!(got_spawn, "should spawn dropped item");
    }

    // ── Container Q-drop ───────────────────────────────────────

    #[test]
    fn container_q_drop_spawns_item() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CHEST);
        let mut be = basalt_world::block_entity::BlockEntity::empty_chest();
        let basalt_world::block_entity::BlockEntity::Chest { ref mut slots } = be;
        slots[0] = Slot::new(1, 10);
        game_loop.world.set_block_entity(5, 64, 3, be);

        let _ = game_tx.send(GameInput::BlockPlace {
            uuid,
            x: 5,
            y: 64,
            z: 3,
            direction: 1,
            sequence: 1,
        });
        game_loop.tick(1);
        while rx.try_recv().is_ok() {}

        // Q key on chest slot 0 (mode 4, button 0)
        click(&game_tx, &mut game_loop, uuid, 0, 0, 4);

        let chest_be = game_loop.world.get_block_entity(5, 64, 3).unwrap();
        match &*chest_be {
            basalt_world::block_entity::BlockEntity::Chest { slots } => {
                assert_eq!(slots[0].item_count, 9);
            }
        }
    }
}
