//! Shift-click processors and helpers for moving items between inventory sections.

use basalt_types::Uuid;

use super::super::slots::WindowType;
use crate::game::GameLoop;
use crate::game::click::WindowSlot;

impl GameLoop {
    /// Processes a shift-click on a slot.
    ///
    /// Returns true if any crafting grid slot was modified.
    pub(in crate::game::inventory) fn process_shift_click(
        &mut self,
        uuid: Uuid,
        eid: basalt_ecs::EntityId,
        ws: WindowSlot,
        wt: &WindowType,
        container_pos: Option<(i32, i32, i32)>,
    ) -> bool {
        match &ws {
            WindowSlot::CraftOutput => {
                let has_output = self
                    .ecs
                    .get::<basalt_api::components::CraftingGrid>(eid)
                    .is_some_and(|g| !g.output.is_empty());
                if !has_output {
                    return false;
                }
                let cancelled = self.dispatch_crafting_pre_craft(uuid, eid, true);
                if !cancelled {
                    self.handle_shift_click_craft(uuid, eid);
                    self.sync_crafting_grid_to_client(eid);
                    self.sync_inventory_to_client(eid);
                }
                true
            }
            WindowSlot::CraftGrid(_) => {
                let grid_item = self.read_slot(eid, &ws, None);
                if grid_item.is_empty() {
                    return false;
                }
                let item_id = grid_item.item_id.unwrap();
                let count = grid_item.item_count;
                let inserted = self
                    .ecs
                    .get_mut::<basalt_api::components::Inventory>(eid)
                    .and_then(|inv| inv.try_insert(item_id, count))
                    .is_some();
                if inserted {
                    self.write_slot(eid, &ws, basalt_types::Slot::empty(), None);
                    self.sync_slot(eid, wt, &ws, basalt_types::Slot::empty());
                    self.sync_inventory_to_client(eid);
                }
                true
            }
            WindowSlot::Container(i) => {
                let item = self.read_slot(eid, &ws, container_pos);
                if item.is_empty() {
                    return false;
                }
                let item_id = item.item_id.unwrap();
                let count = item.item_count;
                let inserted = self
                    .ecs
                    .get_mut::<basalt_api::components::Inventory>(eid)
                    .and_then(|inv| inv.try_insert(item_id, count))
                    .is_some();
                if inserted {
                    self.write_slot(eid, &ws, basalt_types::Slot::empty(), container_pos);
                    self.sync_slot(eid, wt, &ws, basalt_types::Slot::empty());
                    self.sync_inventory_to_client(eid);
                    self.dispatch_container_slot_changed(
                        uuid,
                        eid,
                        wt,
                        *i as i16,
                        item,
                        basalt_types::Slot::empty(),
                    );
                }
                false
            }
            WindowSlot::MainInventory(_) | WindowSlot::Hotbar(_) => {
                self.process_shift_click_inventory(uuid, eid, &ws, wt)
            }
            WindowSlot::Armor(_) => false,
            WindowSlot::Offhand => false,
        }
    }

    /// Shift-clicks an inventory slot to the opposite section or container.
    ///
    /// If a container is open, tries to insert into the container.
    /// Otherwise moves between main inventory and hotbar.
    /// Returns true if a craft grid slot was modified (never for inv slots).
    fn process_shift_click_inventory(
        &mut self,
        uuid: Uuid,
        eid: basalt_ecs::EntityId,
        ws: &WindowSlot,
        wt: &WindowType,
    ) -> bool {
        let item = self.read_slot(eid, ws, None);
        if item.is_empty() {
            return false;
        }
        let item_id = item.item_id.unwrap();
        let count = item.item_count;

        match wt {
            WindowType::Chest {
                container_size,
                position,
                ..
            } => {
                // Try to insert into container block entity
                if self.try_insert_into_container(*position, item_id, count, *container_size) {
                    self.write_slot(eid, ws, basalt_types::Slot::empty(), None);
                    self.sync_slot(eid, wt, ws, basalt_types::Slot::empty());
                    // Re-sync the full container to this player and viewers
                    self.sync_container_to_client(eid, *position, wt);
                    // Notify that the block entity was modified
                    self.notify_block_entity_modified(
                        uuid, eid, position.0, position.1, position.2,
                    );
                }
            }
            _ => {
                // Move between main <-> hotbar
                let target_range = match ws {
                    WindowSlot::MainInventory(_) => 0..9usize, // -> hotbar
                    WindowSlot::Hotbar(_) => 9..36usize,       // -> main
                    _ => return false,
                };
                let inserted = self.try_insert_into_range(eid, item_id, count, target_range);
                if inserted {
                    self.write_slot(eid, ws, basalt_types::Slot::empty(), None);
                    self.sync_inventory_to_client(eid);
                }
            }
        }
        false
    }

    /// Tries to insert an item into the container at the given position.
    ///
    /// Returns true if the full amount was inserted.
    fn try_insert_into_container(
        &mut self,
        pos: (i32, i32, i32),
        item_id: i32,
        count: i32,
        _container_size: usize,
    ) -> bool {
        let view = self.build_chest_view(pos.0, pos.1, pos.2);
        // Try stacking first, then empty slots
        for pass in 0..2 {
            for part in &view.parts {
                for idx in 0..part.slot_count {
                    let s = self.read_container_slot(part.position, idx);
                    if pass == 0 {
                        if s.item_id == Some(item_id) && s.item_count < 64 {
                            let space = 64 - s.item_count;
                            if space >= count {
                                self.write_container_slot(
                                    part.position,
                                    idx,
                                    basalt_types::Slot::new(item_id, s.item_count + count),
                                );
                                return true;
                            }
                        }
                    } else if s.is_empty() {
                        self.write_container_slot(
                            part.position,
                            idx,
                            basalt_types::Slot::new(item_id, count),
                        );
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Tries to insert an item into a specific range of inventory slots.
    ///
    /// Returns true if the full amount was inserted.
    fn try_insert_into_range(
        &mut self,
        eid: basalt_ecs::EntityId,
        item_id: i32,
        count: i32,
        range: std::ops::Range<usize>,
    ) -> bool {
        let Some(inv) = self.ecs.get_mut::<basalt_api::components::Inventory>(eid) else {
            return false;
        };
        // Try stacking first
        for i in range.clone() {
            if inv.slots[i].item_id == Some(item_id) && inv.slots[i].item_count < 64 {
                let space = 64 - inv.slots[i].item_count;
                if space >= count {
                    inv.slots[i].item_count += count;
                    return true;
                }
            }
        }
        // Then empty slots
        for i in range {
            if inv.slots[i].is_empty() {
                inv.slots[i] = basalt_types::Slot::new(item_id, count);
                return true;
            }
        }
        false
    }

    /// Syncs all container slots to the player after a shift-click insert.
    fn sync_container_to_client(
        &self,
        eid: basalt_ecs::EntityId,
        pos: (i32, i32, i32),
        wt: &WindowType,
    ) {
        let view = self.build_chest_view(pos.0, pos.1, pos.2);
        for i in 0..view.size {
            let ws = WindowSlot::Container(i);
            let item = self.read_slot(eid, &ws, Some(pos));
            self.sync_slot(eid, wt, &ws, item.clone());
            self.notify_container_viewers(pos, eid, i as i16, &item);
        }
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

    // ── Shift-click ────────────────────────────────────────────

    #[test]
    fn shift_click_hotbar_to_main() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_api::components::Inventory>(eid)
            .unwrap()
            .slots[0] = Slot::new(1, 10);

        // Shift-click hotbar 0 (window 36)
        click(&game_tx, &mut game_loop, uuid, 36, 0, 1);

        let inv = game_loop
            .ecs
            .get::<basalt_api::components::Inventory>(eid)
            .unwrap();
        assert!(inv.slots[0].is_empty(), "hotbar should be empty");
        let main_total: i32 = inv.slots[9..36]
            .iter()
            .filter(|s| s.item_id == Some(1))
            .map(|s| s.item_count)
            .sum();
        assert_eq!(main_total, 10, "item should be in main inventory");
    }

    #[test]
    fn shift_click_main_to_hotbar() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_api::components::Inventory>(eid)
            .unwrap()
            .slots[9] = Slot::new(1, 10);

        // Shift-click main slot 9 (window 9)
        click(&game_tx, &mut game_loop, uuid, 9, 0, 1);

        let inv = game_loop
            .ecs
            .get::<basalt_api::components::Inventory>(eid)
            .unwrap();
        assert!(inv.slots[9].is_empty(), "main slot should be empty");
        let hotbar_total: i32 = inv.slots[0..9]
            .iter()
            .filter(|s| s.item_id == Some(1))
            .map(|s| s.item_count)
            .sum();
        assert_eq!(hotbar_total, 10, "item should be in hotbar");
    }

    #[test]
    fn shift_click_crafting_output_batch() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        if let Some(grid) = game_loop
            .ecs
            .get_mut::<basalt_api::components::CraftingGrid>(eid)
        {
            grid.slots[0] = Slot::new(43, 2);
            grid.slots[1] = Slot::new(43, 2);
            grid.slots[3] = Slot::new(43, 2);
            grid.slots[4] = Slot::new(43, 2);
            grid.output = Slot::new(314, 1);
        }

        // Shift-click output (slot 0, mode 1)
        click(&game_tx, &mut game_loop, uuid, 0, 0, 1);

        let grid = game_loop
            .ecs
            .get::<basalt_api::components::CraftingGrid>(eid)
            .unwrap();
        assert!(grid.slots[0].is_empty(), "all planks should be consumed");
        let inv = game_loop
            .ecs
            .get::<basalt_api::components::Inventory>(eid)
            .unwrap();
        let total: i32 = inv
            .slots
            .iter()
            .filter(|s| s.item_id == Some(314))
            .map(|s| s.item_count)
            .sum();
        assert_eq!(total, 2, "should have crafted 2 tables");
    }
}
