//! Double-click collector for gathering matching items onto the cursor.

use basalt_types::Uuid;

use super::super::slots::WindowType;
use crate::game::GameLoop;
use crate::game::click::WindowSlot;
use crate::game::click_handler;

impl GameLoop {
    /// Collects matching items onto the cursor (double-click).
    ///
    /// Returns true if any crafting grid slot was modified.
    pub(in crate::game::inventory) fn process_double_click(
        &mut self,
        uuid: Uuid,
        eid: basalt_ecs::EntityId,
        wt: &WindowType,
        container_pos: Option<(i32, i32, i32)>,
    ) -> bool {
        let cursor = self
            .ecs
            .get::<basalt_core::Inventory>(eid)
            .map(|inv| inv.cursor.clone())
            .unwrap_or_default();
        if cursor.is_empty() {
            return false;
        }

        let (all_items, positions) = self.read_all_slots(eid, wt, container_pos);
        let click_handler::CollectResult {
            updates,
            cursor: new_cursor,
        } = click_handler::collect_double_click(&cursor, &all_items);

        let mut grid_changed = false;
        for (i, update) in updates.iter().enumerate() {
            if let Some(new_slot) = update {
                let ws = &positions[i];
                let old = all_items[i].clone();
                self.write_slot(eid, ws, new_slot.clone(), container_pos);
                self.sync_slot(eid, wt, ws, new_slot.clone());
                if Self::is_craft_slot(ws) {
                    grid_changed = true;
                }
                if let WindowSlot::Container(ci) = ws {
                    self.dispatch_container_slot_changed(
                        uuid,
                        eid,
                        wt,
                        *ci as i16,
                        old,
                        new_slot.clone(),
                    );
                }
            }
        }
        if let Some(inv) = self.ecs.get_mut::<basalt_core::Inventory>(eid) {
            inv.cursor = new_cursor;
        }

        grid_changed
    }
}

#[cfg(test)]
mod tests {
    use basalt_types::{Slot, Uuid};

    use crate::game::tests::{connect_player, test_game_loop};
    use crate::messages::GameInput;

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

    // ── Double-click collect ───────────────────────────────────

    #[test]
    fn double_click_collects_matching() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        {
            let inv = game_loop
                .ecs
                .get_mut::<basalt_core::Inventory>(eid)
                .unwrap();
            inv.cursor = Slot::new(1, 5);
            inv.slots[9] = Slot::new(1, 10);
            inv.slots[10] = Slot::new(1, 8);
            inv.slots[11] = Slot::new(2, 3); // different item
        }

        // Double-click (mode 6)
        click(&game_tx, &mut game_loop, uuid, 9, 0, 6);

        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert_eq!(
            inv.cursor.item_count, 23,
            "cursor should collect all matching"
        );
        assert!(inv.slots[9].is_empty(), "slot 9 should be drained");
        assert!(inv.slots[10].is_empty(), "slot 10 should be drained");
        assert_eq!(inv.slots[11].item_id, Some(2), "different item untouched");
    }
}
