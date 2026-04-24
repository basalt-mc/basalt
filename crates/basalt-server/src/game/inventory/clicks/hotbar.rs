//! Hotbar swap processor for number-key slot swaps.

use basalt_types::Uuid;

use super::super::slots::WindowType;
use crate::game::GameLoop;
use crate::game::click::WindowSlot;

impl GameLoop {
    /// Swaps a slot with a hotbar slot (number keys 1-9).
    ///
    /// Returns true if a crafting grid slot was modified.
    pub(in crate::game::inventory) fn process_hotbar_swap(
        &mut self,
        uuid: Uuid,
        eid: basalt_ecs::EntityId,
        ws: WindowSlot,
        hotbar: u8,
        wt: &WindowType,
        container_pos: Option<(i32, i32, i32)>,
    ) -> bool {
        let clicked_item = self.read_slot(eid, &ws, container_pos);
        let hotbar_ws = WindowSlot::Hotbar(hotbar as usize);
        let hotbar_item = self.read_slot(eid, &hotbar_ws, None);

        self.write_slot(eid, &ws, hotbar_item.clone(), container_pos);
        self.write_slot(eid, &hotbar_ws, clicked_item.clone(), None);

        self.sync_slot(eid, wt, &ws, hotbar_item.clone());
        self.sync_slot(eid, wt, &hotbar_ws, clicked_item.clone());

        if let WindowSlot::Container(i) = &ws {
            if let Some(pos) = container_pos {
                self.notify_container_viewers(pos, eid, *i as i16, &hotbar_item);
            }
            self.dispatch_container_slot_changed(
                uuid,
                eid,
                wt,
                *i as i16,
                clicked_item,
                hotbar_item,
            );
        }

        Self::is_craft_slot(&ws)
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

    // ── Hotbar swap ────────────────────────────────────────────

    #[test]
    fn hotbar_swap() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        {
            let inv = game_loop
                .ecs
                .get_mut::<basalt_core::Inventory>(eid)
                .unwrap();
            inv.slots[9] = Slot::new(1, 10); // main slot
            inv.slots[2] = Slot::new(2, 5); // hotbar slot 2
        }

        // Swap main slot 9 (window 9) with hotbar 2 (mode 2, button 2)
        click(&game_tx, &mut game_loop, uuid, 9, 2, 2);

        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert_eq!(
            inv.slots[9].item_id,
            Some(2),
            "main should have hotbar item"
        );
        assert_eq!(
            inv.slots[2].item_id,
            Some(1),
            "hotbar should have main item"
        );
    }
}
