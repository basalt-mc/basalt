//! Left-click and right-click on a single slot.
//!
//! Handles pick up, place, swap, stack (left), and half-pick,
//! place-one (right) using the pure functions in [`click_handler`].

use basalt_types::Uuid;

use super::super::slots::WindowType;
use crate::game::GameLoop;
use crate::game::click::WindowSlot;
use crate::game::click_handler;

impl GameLoop {
    /// Processes a left or right click on a single slot.
    ///
    /// Reads the clicked slot and cursor, computes the result via
    /// [`click_handler::left_click`] or [`click_handler::right_click`],
    /// writes back, and syncs to the client.
    ///
    /// Returns true if any crafting grid slot was modified.
    pub(in crate::game::inventory) fn process_simple_click(
        &mut self,
        uuid: Uuid,
        eid: basalt_ecs::EntityId,
        ws: WindowSlot,
        is_right: bool,
        wt: &WindowType,
        container_pos: Option<(i32, i32, i32)>,
    ) -> bool {
        // Craft output: special handling in craft_output.rs
        if matches!(ws, WindowSlot::CraftOutput) {
            return self.process_craft_output_click(uuid, eid, wt);
        }

        let slot_item = self.read_slot(eid, &ws, container_pos);
        let cursor = self
            .ecs
            .get::<basalt_api::components::Inventory>(eid)
            .map(|inv| inv.cursor.clone())
            .unwrap_or_default();

        let click_handler::ClickResult {
            clicked: new_slot,
            cursor: new_cursor,
        } = if is_right {
            click_handler::right_click(&slot_item, &cursor)
        } else {
            click_handler::left_click(&slot_item, &cursor)
        };

        self.write_slot(eid, &ws, new_slot.clone(), container_pos);
        if let Some(inv) = self.ecs.get_mut::<basalt_api::components::Inventory>(eid) {
            inv.cursor = new_cursor;
        }
        self.sync_slot(eid, wt, &ws, new_slot.clone());

        // Dispatch ContainerSlotChangedEvent — `ContainerPlugin` listens at
        // Post and notifies co-viewers via `ctx.containers().notify_viewers`.
        if let WindowSlot::Container(i) = &ws {
            let proto_slot = *i as i16;
            self.dispatch_container_slot_changed(uuid, eid, wt, proto_slot, slot_item, new_slot);
        }

        Self::is_craft_slot(&ws)
    }

    /// Takes the crafting output and consumes ingredients.
    ///
    /// Validates that the cursor is empty or compatible with the output,
    /// dispatches `CraftingPreCraftEvent` (allowing cancellation),
    /// then moves the output to the cursor and decrements each grid
    /// ingredient by one.
    ///
    /// Returns true (crafting grid always changes when output is taken).
    pub(in crate::game::inventory) fn process_craft_output_click(
        &mut self,
        uuid: Uuid,
        eid: basalt_ecs::EntityId,
        _wt: &WindowType,
    ) -> bool {
        let output = self
            .ecs
            .get::<basalt_api::components::CraftingGrid>(eid)
            .map(|g| g.output.clone())
            .unwrap_or_default();
        if output.is_empty() {
            return false;
        }

        let cursor = self
            .ecs
            .get::<basalt_api::components::Inventory>(eid)
            .map(|inv| inv.cursor.clone())
            .unwrap_or_default();

        // Can only take if cursor is empty or same item with space
        if !cursor.is_empty()
            && (cursor.item_id != output.item_id || cursor.item_count + output.item_count > 64)
        {
            return false;
        }

        let cancelled = self.dispatch_crafting_pre_craft(uuid, eid, false);
        if cancelled {
            return false;
        }

        // Snapshot grid contents BEFORE consume so the CraftedEvent
        // carries the pre-consumption state.
        let consumed = self
            .ecs
            .get::<basalt_api::components::CraftingGrid>(eid)
            .map(|g| g.slots.clone())
            .unwrap_or_else(|| std::array::from_fn(|_| basalt_types::Slot::empty()));
        let produced = output.clone();

        if let Some(inv) = self.ecs.get_mut::<basalt_api::components::Inventory>(eid) {
            if inv.cursor.is_empty() {
                inv.cursor = output;
            } else {
                inv.cursor.item_count += output.item_count;
            }
        }

        self.consume_crafting_ingredients(eid);
        self.sync_crafting_grid_to_client(eid);

        // Notify plugins of the successful craft.
        self.dispatch_crafting_crafted(uuid, eid, consumed, produced);

        // Re-match recipe after consuming — return true so caller
        // dispatches grid changed + updates output
        true
    }
}

#[cfg(test)]
mod tests {
    use basalt_types::{Slot, Uuid};

    use crate::game::tests::{connect_player, test_game_loop};
    use crate::messages::{GameInput, ServerOutput};

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

    // ── Left-click ─────────────────────────────────────────────

    #[test]
    fn left_click_pick_up_from_inventory() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_api::components::Inventory>(eid)
            .unwrap()
            .slots[0] = Slot::new(1, 10);
        while rx.try_recv().is_ok() {}

        // Left-click hotbar slot 0 (window slot 36)
        click(&game_tx, &mut game_loop, uuid, 36, 0, 0);

        let inv = game_loop
            .ecs
            .get::<basalt_api::components::Inventory>(eid)
            .unwrap();
        assert!(inv.slots[0].is_empty(), "slot should be empty after pickup");
        assert_eq!(inv.cursor.item_id, Some(1));
        assert_eq!(inv.cursor.item_count, 10);
    }

    #[test]
    fn left_click_place_into_empty() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_api::components::Inventory>(eid)
            .unwrap()
            .cursor = Slot::new(1, 10);

        // Left-click empty main slot 9 (window slot 9)
        click(&game_tx, &mut game_loop, uuid, 9, 0, 0);

        let inv = game_loop
            .ecs
            .get::<basalt_api::components::Inventory>(eid)
            .unwrap();
        assert_eq!(inv.slots[9].item_id, Some(1));
        assert_eq!(inv.slots[9].item_count, 10);
        assert!(inv.cursor.is_empty());
    }

    #[test]
    fn left_click_swap_items() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        {
            let inv = game_loop
                .ecs
                .get_mut::<basalt_api::components::Inventory>(eid)
                .unwrap();
            inv.slots[0] = Slot::new(1, 10);
            inv.cursor = Slot::new(2, 5);
        }

        click(&game_tx, &mut game_loop, uuid, 36, 0, 0);

        let inv = game_loop
            .ecs
            .get::<basalt_api::components::Inventory>(eid)
            .unwrap();
        assert_eq!(inv.slots[0].item_id, Some(2));
        assert_eq!(inv.slots[0].item_count, 5);
        assert_eq!(inv.cursor.item_id, Some(1));
        assert_eq!(inv.cursor.item_count, 10);
    }

    #[test]
    fn left_click_stack_same_items() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        {
            let inv = game_loop
                .ecs
                .get_mut::<basalt_api::components::Inventory>(eid)
                .unwrap();
            inv.slots[0] = Slot::new(1, 30);
            inv.cursor = Slot::new(1, 20);
        }

        click(&game_tx, &mut game_loop, uuid, 36, 0, 0);

        let inv = game_loop
            .ecs
            .get::<basalt_api::components::Inventory>(eid)
            .unwrap();
        assert_eq!(inv.slots[0].item_count, 50);
        assert!(inv.cursor.is_empty());
    }

    // ── Right-click ────────────────────────────────────────────

    #[test]
    fn right_click_pick_up_half() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_api::components::Inventory>(eid)
            .unwrap()
            .slots[0] = Slot::new(1, 10);

        click(&game_tx, &mut game_loop, uuid, 36, 1, 0);

        let inv = game_loop
            .ecs
            .get::<basalt_api::components::Inventory>(eid)
            .unwrap();
        assert_eq!(inv.slots[0].item_count, 5);
        assert_eq!(inv.cursor.item_count, 5);
    }

    #[test]
    fn right_click_place_one() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_api::components::Inventory>(eid)
            .unwrap()
            .cursor = Slot::new(1, 10);

        click(&game_tx, &mut game_loop, uuid, 9, 1, 0);

        let inv = game_loop
            .ecs
            .get::<basalt_api::components::Inventory>(eid)
            .unwrap();
        assert_eq!(inv.slots[9].item_id, Some(1));
        assert_eq!(inv.slots[9].item_count, 1);
        assert_eq!(inv.cursor.item_count, 9);
    }

    // ── Chest interaction ──────────────────────────────────────

    #[test]
    fn chest_click_moves_item() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        game_loop
            .world
            .set_block(5, 64, 3, basalt_api::world::block::CHEST);
        game_loop.world.set_block_entity(
            5,
            64,
            3,
            basalt_api::world::block_entity::BlockEntity::empty_chest(),
        );
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

        game_loop
            .ecs
            .get_mut::<basalt_api::components::Inventory>(eid)
            .unwrap()
            .cursor = Slot::new(1, 10);

        click(&game_tx, &mut game_loop, uuid, 0, 0, 0);

        let be = game_loop.world.get_block_entity(5, 64, 3).unwrap();
        match &*be {
            basalt_api::world::block_entity::BlockEntity::Chest { slots } => {
                assert_eq!(slots[0].item_id, Some(1));
                assert_eq!(slots[0].item_count, 10);
            }
        }

        let inv = game_loop
            .ecs
            .get::<basalt_api::components::Inventory>(eid)
            .unwrap();
        assert!(inv.cursor.is_empty());
    }

    // ── Cursor sync ────────────────────────────────────────────

    #[test]
    fn cursor_synced_after_every_click() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_api::components::Inventory>(eid)
            .unwrap()
            .slots[0] = Slot::new(1, 10);
        while rx.try_recv().is_ok() {}

        click(&game_tx, &mut game_loop, uuid, 36, 0, 0);

        use basalt_mc_protocol::packets::play::inventory::ClientboundPlaySetSlot;
        let mut got_cursor = false;
        while let Ok(msg) = rx.try_recv() {
            if let ServerOutput::Plain(ep) = &msg
                && let Some(p) = ep.downcast::<ClientboundPlaySetSlot>()
                && p.window_id == -1
                && p.slot == -1
            {
                got_cursor = true;
            }
        }
        assert!(got_cursor, "cursor should be synced after click");
    }

    // ── Craft output (left-click on output slot) ───────────────

    #[test]
    fn craft_output_click_takes_result() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .world
            .set_block(5, 64, 3, basalt_api::world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        if let Some(grid) = game_loop
            .ecs
            .get_mut::<basalt_api::components::CraftingGrid>(eid)
        {
            grid.slots[0] = Slot::new(43, 1);
            grid.slots[1] = Slot::new(43, 1);
            grid.slots[3] = Slot::new(43, 1);
            grid.slots[4] = Slot::new(43, 1);
            grid.output = Slot::new(314, 1);
        }

        click(&game_tx, &mut game_loop, uuid, 0, 0, 0);

        let grid = game_loop
            .ecs
            .get::<basalt_api::components::CraftingGrid>(eid)
            .unwrap();
        assert!(grid.slots[0].is_empty());
        assert!(grid.slots[1].is_empty());

        let inv = game_loop
            .ecs
            .get::<basalt_api::components::Inventory>(eid)
            .unwrap();
        assert_eq!(inv.cursor.item_id, Some(314));
    }
}
