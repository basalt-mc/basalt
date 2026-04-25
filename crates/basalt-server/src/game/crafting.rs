//! Crafting logic — crafting table and player inventory grid interactions.
//!
//! Handles recipe matching, ingredient consumption, shift-click crafting,
//! and crafting window management. All crafting-specific methods live here
//! to keep `inventory.rs` and `container.rs` focused on their domains.

use basalt_api::events::{
    CraftingGridChangedEvent, CraftingPreCraftEvent, CraftingRecipeClearedEvent,
    CraftingRecipeMatchedEvent,
};
use basalt_types::Slot;
use basalt_types::Uuid;

use super::GameLoop;
use crate::messages::ServerOutput;

impl GameLoop {
    /// Opens a crafting table window (3x3 grid) for a player.
    ///
    /// Switches the player's `CraftingGrid` component to 3x3 mode,
    /// assigns a window ID, sets `OpenContainer` for close handling,
    /// and sends `OpenWindow` with inventory type 11 (crafting table).
    pub(super) fn open_crafting_table(
        &mut self,
        eid: basalt_ecs::EntityId,
        x: i32,
        y: i32,
        z: i32,
    ) {
        // Switch CraftingGrid to 3x3 mode and clear
        if let Some(grid) = self.ecs.get_mut::<basalt_core::CraftingGrid>(eid) {
            grid.grid_size = 3;
            grid.clear();
        }

        let window_id = self.alloc_window_id();

        // Set OpenContainer so window close is handled
        self.ecs.set(
            eid,
            basalt_core::OpenContainer {
                window_id,
                inventory_type: basalt_core::InventoryType::Crafting,
                backing: basalt_core::ContainerBacking::Block {
                    position: basalt_core::BlockPosition { x, y, z },
                },
            },
        );

        // Build window slots: output (1) + 3x3 grid (9) + main inv (27) + hotbar (9) = 46
        let mut window_slots = Vec::with_capacity(46);

        // Output slot (empty initially)
        window_slots.push(basalt_types::Slot::empty());

        // 3x3 crafting grid (empty)
        for _ in 0..9 {
            window_slots.push(basalt_types::Slot::empty());
        }

        // Player inventory (main + hotbar)
        if let Some(inv) = self.ecs.get::<basalt_core::Inventory>(eid) {
            window_slots.extend_from_slice(&inv.slots[9..]); // main (27)
            window_slots.extend_from_slice(&inv.slots[..9]); // hotbar (9)
        }

        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::OpenWindow {
                window_id,
                inventory_type: 11, // crafting table
                title: basalt_types::TextComponent::text("Crafting").to_nbt(),
                slots: window_slots,
            });
        });
    }

    /// Dispatches a `CraftingGridChangedEvent` for the given player.
    ///
    /// Builds the grid array from the `CraftingGrid` component and
    /// fires the event through the game bus. The crafting plugin
    /// responds by computing and setting the output slot.
    pub(super) fn dispatch_crafting_grid_changed(&mut self, uuid: Uuid, eid: basalt_ecs::EntityId) {
        let (grid, grid_size) = {
            let Some(grid_comp) = self.ecs.get::<basalt_core::CraftingGrid>(eid) else {
                return;
            };
            let gs = grid_comp.grid_size;
            let slot_count = (gs as usize) * (gs as usize);
            let mut g = [None; 9];
            for (i, slot) in grid_comp.slots.iter().enumerate().take(slot_count.min(9)) {
                g[i] = slot.item_id;
            }
            (g, gs)
        };

        let (entity_id, username, yaw, pitch) = match self.player_info(eid) {
            Some(info) => info,
            None => return,
        };

        let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);
        let mut event = CraftingGridChangedEvent { grid, grid_size };
        self.dispatch_event(&mut event, &ctx);
        self.process_responses(uuid, &ctx.drain_responses());
    }

    /// Dispatches a `CraftingPreCraftEvent` for the given player.
    ///
    /// Fires at Validate when the player clicks the output slot of a
    /// crafting grid that has a valid recipe result. Returns `true`
    /// if the event was cancelled by a Validate handler — the caller
    /// must abort the craft (no consume, no transfer).
    pub(super) fn dispatch_crafting_pre_craft(
        &mut self,
        uuid: Uuid,
        eid: basalt_ecs::EntityId,
        is_shift_click: bool,
    ) -> bool {
        let result: Slot = {
            let Some(grid) = self.ecs.get::<basalt_core::CraftingGrid>(eid) else {
                return true;
            };
            if grid.output.item_id.is_none() {
                return true;
            }
            grid.output.clone()
        };

        let (entity_id, username, yaw, pitch) = match self.player_info(eid) {
            Some(info) => info,
            None => return true,
        };

        let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);
        let mut event = CraftingPreCraftEvent {
            result,
            is_shift_click,
            cancelled: false,
        };
        self.dispatch_event(&mut event, &ctx);
        self.process_responses(uuid, &ctx.drain_responses());
        event.cancelled
    }

    /// Resolves the crafting result for the current grid contents.
    ///
    /// Calls `RecipeRegistry::match_grid` and dispatches the
    /// appropriate event:
    /// - `CraftingRecipeMatchedEvent` (Process+Post, mutable `result`)
    ///   when a recipe matches. Plugins layer modifications by handler
    ///   priority — setting `result` to `Slot::empty()` denies the
    ///   craft.
    /// - `CraftingRecipeClearedEvent` (Post) only on the transition
    ///   `matched → unmatched` (i.e. when `grid.output` was non-empty
    ///   before this call and no recipe now matches).
    ///
    /// Returns the final (post-mutation) result slot. Does **not**
    /// write to `grid.output` and does **not** send any packet — see
    /// [`sync_crafting_output_slot`](Self::sync_crafting_output_slot)
    /// and [`run_crafting_match_cycle`](Self::run_crafting_match_cycle).
    pub(super) fn compute_crafting_match(&mut self, uuid: Uuid, eid: basalt_ecs::EntityId) -> Slot {
        let (grid_ids, grid_size, previous_output) = {
            let Some(grid) = self.ecs.get::<basalt_core::CraftingGrid>(eid) else {
                return Slot::empty();
            };
            let mut g = [None; 9];
            for (i, slot) in grid.slots.iter().enumerate().take(9) {
                g[i] = slot.item_id;
            }
            (g, grid.grid_size, grid.output.clone())
        };

        let raw_match = self
            .recipes
            .match_grid(&grid_ids, grid_size)
            .map(|(id, count)| Slot::new(id, count));

        let (entity_id, username, yaw, pitch) = match self.player_info(eid) {
            Some(info) => info,
            None => return raw_match.unwrap_or_else(Slot::empty),
        };

        let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);

        match raw_match {
            Some(initial) => {
                let mut event = CraftingRecipeMatchedEvent {
                    grid: grid_ids,
                    grid_size,
                    result: initial,
                };
                self.dispatch_event(&mut event, &ctx);
                self.process_responses(uuid, &ctx.drain_responses());
                event.result
            }
            None => {
                if !previous_output.is_empty() {
                    let mut event = CraftingRecipeClearedEvent { grid_size };
                    self.dispatch_event(&mut event, &ctx);
                    self.process_responses(uuid, &ctx.drain_responses());
                }
                Slot::empty()
            }
        }
    }

    /// Writes the resolved result to `CraftingGrid.output` and pushes
    /// a `SetContainerSlot` for slot 0 to the client.
    ///
    /// Always sends because crafting output is server-authoritative:
    /// the client clears slot 0 locally when the player takes the
    /// output, so the server must re-send even when the same recipe
    /// still matches.
    pub(super) fn sync_crafting_output_slot(&mut self, eid: basalt_ecs::EntityId, output: Slot) {
        if let Some(grid) = self.ecs.get_mut::<basalt_core::CraftingGrid>(eid) {
            grid.output = output.clone();
        }

        // Window ID 0 = player inventory window (used for the 2x2 grid
        // when no container is open). The client accepts slot 0
        // updates because crafting output is always server-pushed.
        let window_id = self
            .ecs
            .get::<basalt_core::OpenContainer>(eid)
            .map(|oc| i32::from(oc.window_id))
            .unwrap_or(0);

        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::SetContainerSlot {
                window_id,
                slot: 0,
                item: output,
            });
        });
    }

    /// Runs the full crafting match cycle: resolve match (firing
    /// matched/cleared events) and sync slot 0 to the client.
    ///
    /// This is the steady-state entry point invoked after every
    /// crafting-grid mutation in the inventory click handler.
    pub(super) fn run_crafting_match_cycle(&mut self, uuid: Uuid, eid: basalt_ecs::EntityId) {
        let output = self.compute_crafting_match(uuid, eid);
        self.sync_crafting_output_slot(eid, output);
    }

    /// Consumes one ingredient from each occupied grid slot after
    /// the player takes the crafting output.
    ///
    /// Decrements `item_count` by 1 for every non-empty slot. Slots
    /// that reach zero are replaced with `Slot::empty()`. The output
    /// slot is also cleared since the result was consumed.
    pub(super) fn consume_crafting_ingredients(&mut self, eid: basalt_ecs::EntityId) {
        let Some(grid) = self.ecs.get_mut::<basalt_core::CraftingGrid>(eid) else {
            return;
        };
        let slot_count = (grid.grid_size as usize) * (grid.grid_size as usize);
        for slot in grid.slots.iter_mut().take(slot_count.min(9)) {
            if slot.item_id.is_some() {
                slot.item_count -= 1;
                if slot.item_count <= 0 {
                    *slot = basalt_types::Slot::empty();
                }
            }
        }
        grid.output = basalt_types::Slot::empty();
    }

    /// Sends updated crafting grid slots to the client after
    /// ingredient consumption.
    ///
    /// Each grid slot is sent individually via `SetContainerSlot`.
    /// Grid slots are 1-based in the window (slot 0 is the output).
    pub(super) fn sync_crafting_grid_to_client(&mut self, eid: basalt_ecs::EntityId) {
        let (slots_data, slot_count, window_id) = {
            let Some(grid) = self.ecs.get::<basalt_core::CraftingGrid>(eid) else {
                return;
            };
            let wid = self
                .ecs
                .get::<basalt_core::OpenContainer>(eid)
                .map(|oc| i32::from(oc.window_id))
                .unwrap_or(0);
            let sc = (grid.grid_size as usize) * (grid.grid_size as usize);
            let data: Vec<basalt_types::Slot> =
                grid.slots.iter().take(sc.min(9)).cloned().collect();
            (data, sc.min(9), wid)
        };

        for (i, item) in slots_data.into_iter().enumerate().take(slot_count) {
            let window_slot = (i + 1) as i16;
            self.send_to(eid, |tx| {
                let _ = tx.try_send(ServerOutput::SetContainerSlot {
                    window_id,
                    slot: window_slot,
                    item,
                });
            });
        }
    }

    /// Sends updated inventory slots to the client after shift-click
    /// crafting inserts results into the player's inventory.
    ///
    /// Compares the current inventory against a snapshot taken before
    /// the batch craft and only sends `SetContainerSlot` for slots
    /// that actually changed. Uses the crafting table window layout
    /// (10-36 = main, 37-45 = hotbar) or player inventory layout
    /// (9-35 = main, 36-44 = hotbar) depending on whether an
    /// `OpenContainer` is present.
    pub(super) fn sync_inventory_to_client(&mut self, eid: basalt_ecs::EntityId) {
        let Some(inv) = self.ecs.get::<basalt_core::Inventory>(eid) else {
            return;
        };
        let slots = inv.slots.clone();

        let has_open_container = self.ecs.has::<basalt_core::OpenContainer>(eid);
        let window_id = self
            .ecs
            .get::<basalt_core::OpenContainer>(eid)
            .map(|oc| i32::from(oc.window_id))
            .unwrap_or(0);

        for (i, item) in slots.iter().enumerate() {
            let window_slot = if has_open_container {
                // Crafting table window: hotbar 0-8 → 37-45, main 9-35 → 10-36
                if i < 9 {
                    (i + 37) as i16
                } else {
                    (i + 1) as i16
                }
            } else {
                // Player inventory window: hotbar 0-8 → 36-44, main 9-35 → 9-35
                if i < 9 { (i + 36) as i16 } else { i as i16 }
            };

            let item = item.clone();
            self.send_to(eid, |tx| {
                let _ = tx.try_send(ServerOutput::SetContainerSlot {
                    window_id,
                    slot: window_slot,
                    item,
                });
            });
        }
    }

    /// Handles shift-click crafting: repeatedly consumes ingredients
    /// and inserts results into the player's inventory until either
    /// no recipe matches or the inventory is full.
    ///
    /// Between iterations, calls
    /// [`compute_crafting_match`](Self::compute_crafting_match) to
    /// resolve the next result through the event pipeline so plugins
    /// can mutate the per-iteration result.
    pub(super) fn handle_shift_click_craft(&mut self, uuid: Uuid, eid: basalt_ecs::EntityId) {
        loop {
            // Read the current output (set by the prior iteration's
            // `compute_crafting_match` or by the initial click).
            let (result_id, result_count) = {
                let Some(grid) = self.ecs.get::<basalt_core::CraftingGrid>(eid) else {
                    break;
                };
                let Some(id) = grid.output.item_id else {
                    break;
                };
                (id, grid.output.item_count)
            };

            // Try to insert into inventory
            let inserted = {
                let Some(inv) = self.ecs.get_mut::<basalt_core::Inventory>(eid) else {
                    break;
                };
                inv.try_insert(result_id, result_count).is_some()
            };
            if !inserted {
                break;
            }

            // Consume ingredients (clears grid.output as a side effect)
            self.consume_crafting_ingredients(eid);

            // Resolve next iteration through the event pipeline so
            // plugins observe each match. `compute_crafting_match`
            // suppresses `CraftingRecipeClearedEvent` here because the
            // previous output was already cleared by `consume`.
            let next = self.compute_crafting_match(uuid, eid);
            if let Some(grid) = self.ecs.get_mut::<basalt_core::CraftingGrid>(eid) {
                grid.output = next.clone();
            }
            if next.is_empty() {
                break;
            }
        }
    }

    /// Extracts player info (entity_id, username, yaw, pitch) for event dispatch.
    pub(crate) fn player_info(&self, eid: basalt_ecs::EntityId) -> Option<(i32, String, f32, f32)> {
        let pr = self.ecs.get::<basalt_core::PlayerRef>(eid)?;
        let entity_id = eid as i32;
        let username = pr.username.clone();
        let (yaw, pitch) = self
            .ecs
            .get::<basalt_core::Rotation>(eid)
            .map_or((0.0, 0.0), |r| (r.yaw, r.pitch));
        Some((entity_id, username, yaw, pitch))
    }
}

#[cfg(test)]
mod tests {
    use basalt_types::Uuid;

    use crate::messages::{GameInput, ServerOutput};

    #[test]
    fn crafting_table_grid_click_updates_component() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Place crafting table and open it
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        // Set cursor to item first — server-authoritative left-click
        // places cursor into empty grid slot
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .cursor = basalt_types::Slot::new(43, 1);

        // Left-click on grid slot 1 (window slot 1 = grid index 0)
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 1,
            button: 0,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        // CraftingGrid slot 0 should have the item
        let grid = game_loop.ecs.get::<basalt_core::CraftingGrid>(eid).unwrap();
        assert_eq!(grid.slots[0].item_id, Some(43));
        assert_eq!(grid.slots[0].item_count, 1);
        // Cursor should be empty
        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert!(
            inv.cursor.is_empty(),
            "cursor should be empty after placing"
        );
    }

    #[test]
    fn crafting_table_inventory_slot_updates_player_inv() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Place crafting table and open it
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        // Set cursor item server-side, then left-click to place into
        // hotbar slot (window slot 37 = hotbar 0 = internal 0)
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .cursor = basalt_types::Slot::new(1, 10);

        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 37,
            button: 0,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert_eq!(inv.slots[0].item_id, Some(1));
        assert_eq!(inv.slots[0].item_count, 10);
    }

    #[test]
    fn player_inventory_2x2_grid_updates_component() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Set cursor to item — server-authoritative left-click places
        // cursor into empty grid slot
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .cursor = basalt_types::Slot::new(43, 1);

        // Left-click on 2x2 crafting slot (window slot 1 = grid index 0)
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 1,
            button: 0,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        let grid = game_loop.ecs.get::<basalt_core::CraftingGrid>(eid).unwrap();
        assert_eq!(
            grid.slots[0].item_id,
            Some(43),
            "2x2 grid slot 0 should have the item"
        );
    }

    #[test]
    fn recipe_matching_produces_output() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Open crafting table
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        // Place 4 oak planks (id 43) in the 2x2 top-left of the 3x3 grid.
        // Server-authoritative: set grid directly and trigger recipe match
        // via a single grid click that triggers dispatch_crafting_grid_changed.
        if let Some(grid) = game_loop.ecs.get_mut::<basalt_core::CraftingGrid>(eid) {
            grid.slots[0] = basalt_types::Slot::new(43, 1);
            grid.slots[1] = basalt_types::Slot::new(43, 1);
            grid.slots[3] = basalt_types::Slot::new(43, 1);
        }

        // Place the last plank via a cursor click to trigger recipe matching
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .cursor = basalt_types::Slot::new(43, 1);
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 5,
            button: 0,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        // Output should be crafting table (id 314)
        let grid = game_loop.ecs.get::<basalt_core::CraftingGrid>(eid).unwrap();
        assert_eq!(
            grid.output.item_id,
            Some(314),
            "output should be crafting table"
        );
        assert_eq!(grid.output.item_count, 1);

        // Client should have received SetContainerSlot for the output
        let mut got_output_slot = false;
        while let Ok(msg) = rx.try_recv() {
            if let ServerOutput::SetContainerSlot { slot: 0, item, .. } = &msg
                && item.item_id == Some(314)
            {
                got_output_slot = true;
            }
        }
        assert!(got_output_slot, "client should receive output slot update");
    }

    #[test]
    fn output_click_consumes_ingredients() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Open crafting table
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        // Place 4 oak planks in the grid and set output manually
        // (simulates the state after a grid change + recipe match)
        if let Some(grid) = game_loop.ecs.get_mut::<basalt_core::CraftingGrid>(eid) {
            grid.slots[0] = basalt_types::Slot::new(43, 1);
            grid.slots[1] = basalt_types::Slot::new(43, 1);
            grid.slots[3] = basalt_types::Slot::new(43, 1);
            grid.slots[4] = basalt_types::Slot::new(43, 1);
            grid.output = basalt_types::Slot::new(314, 1);
        }

        // Click the output slot (slot 0, mode 0 = normal click).
        // Server-authoritative: changed_slots are ignored for output clicks.
        // The server computes ingredient consumption itself.
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 0,
            button: 0,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        // All 4 planks should be consumed by server-side logic
        let grid = game_loop.ecs.get::<basalt_core::CraftingGrid>(eid).unwrap();
        assert!(grid.slots[0].is_empty(), "grid slot 0 should be empty");
        assert!(grid.slots[1].is_empty(), "grid slot 1 should be empty");
        assert!(grid.slots[3].is_empty(), "grid slot 3 should be empty");
        assert!(grid.slots[4].is_empty(), "grid slot 4 should be empty");

        // Cursor should hold the crafting result
        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert_eq!(inv.cursor.item_id, Some(314), "cursor should hold result");
        assert_eq!(inv.cursor.item_count, 1);

        // Output should be re-matched (empty grid = no output)
        assert!(
            grid.output.is_empty(),
            "output should be empty after consume"
        );
    }

    #[test]
    fn shift_click_crafts_max() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Open crafting table
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        // Place 2 oak planks in each of the 4 grid slots
        if let Some(grid) = game_loop.ecs.get_mut::<basalt_core::CraftingGrid>(eid) {
            grid.slots[0] = basalt_types::Slot::new(43, 2);
            grid.slots[1] = basalt_types::Slot::new(43, 2);
            grid.slots[3] = basalt_types::Slot::new(43, 2);
            grid.slots[4] = basalt_types::Slot::new(43, 2);
            grid.output = basalt_types::Slot::new(314, 1);
        }

        // Shift-click the output slot (slot 0, mode 1 = shift click)
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 0,
            button: 0,
            mode: 1,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        // Should have crafted 2 times (2 planks per slot, 1 consumed per craft)
        let grid = game_loop.ecs.get::<basalt_core::CraftingGrid>(eid).unwrap();
        assert!(grid.slots[0].is_empty(), "all planks should be consumed");
        assert!(grid.slots[1].is_empty(), "all planks should be consumed");
        assert!(
            grid.output.is_empty(),
            "output should be empty when no more ingredients"
        );

        // Player inventory should contain 2 crafting tables
        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        let total: i32 = inv
            .slots
            .iter()
            .filter(|s| s.item_id == Some(314))
            .map(|s| s.item_count)
            .sum();
        assert_eq!(total, 2, "should have 2 crafting tables in inventory");
    }

    #[test]
    fn no_match_clears_output() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Open crafting table
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        // Set cursor to non-recipe item, then left-click grid slot 1
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .cursor = basalt_types::Slot::new(9999, 1);

        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 1,
            button: 0,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        // Output should be empty (no matching recipe)
        let grid = game_loop.ecs.get::<basalt_core::CraftingGrid>(eid).unwrap();
        assert!(
            grid.output.is_empty(),
            "output should be empty for non-recipe items"
        );
    }

    #[test]
    fn player_inventory_2x2_recipe_matching() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Set 3 planks directly, place the 4th via cursor click to
        // trigger recipe matching through the server-authoritative path
        if let Some(grid) = game_loop.ecs.get_mut::<basalt_core::CraftingGrid>(eid) {
            grid.slots[0] = basalt_types::Slot::new(43, 1);
            grid.slots[1] = basalt_types::Slot::new(43, 1);
            grid.slots[2] = basalt_types::Slot::new(43, 1);
        }

        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .cursor = basalt_types::Slot::new(43, 1);

        // Left-click on 2x2 grid slot 4 (window slot 4 = grid index 3)
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 4,
            button: 0,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        let grid = game_loop.ecs.get::<basalt_core::CraftingGrid>(eid).unwrap();
        assert_eq!(
            grid.output.item_id,
            Some(314),
            "2x2 crafting should match crafting table recipe"
        );
    }

    #[test]
    fn open_crafting_table_sends_window() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Place a crafting table
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);

        // Open the crafting table directly
        game_loop.open_crafting_table(eid, 5, 64, 3);

        // Check OpenWindow was sent with inventory_type 11
        let mut got_open = false;
        let mut inv_type = 0;
        while let Ok(msg) = rx.try_recv() {
            if let ServerOutput::OpenWindow { inventory_type, .. } = &msg {
                got_open = true;
                inv_type = *inventory_type;
            }
        }
        assert!(got_open, "open_crafting_table should send OpenWindow");
        assert_eq!(inv_type, 11, "inventory_type should be 11 (crafting table)");

        // Player should have OpenContainer
        assert!(game_loop.ecs.has::<basalt_core::OpenContainer>(eid));

        // CraftingGrid should be 3x3 mode
        let grid = game_loop.ecs.get::<basalt_core::CraftingGrid>(eid).unwrap();
        assert_eq!(grid.grid_size, 3, "grid should be 3x3 after opening table");
    }

    #[test]
    fn shift_click_stops_when_inventory_full() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Fill all 36 inventory slots with distinct items (no stacking possible)
        if let Some(inv) = game_loop.ecs.get_mut::<basalt_core::Inventory>(eid) {
            for i in 0..36 {
                inv.slots[i] = basalt_types::Slot::new(i as i32 + 100, 64);
            }
        }

        // Open crafting table
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        // Place recipe ingredients in grid (4 oak planks for crafting table)
        if let Some(grid) = game_loop.ecs.get_mut::<basalt_core::CraftingGrid>(eid) {
            grid.slots[0] = basalt_types::Slot::new(43, 2);
            grid.slots[1] = basalt_types::Slot::new(43, 2);
            grid.slots[3] = basalt_types::Slot::new(43, 2);
            grid.slots[4] = basalt_types::Slot::new(43, 2);
            grid.output = basalt_types::Slot::new(314, 1);
        }

        // Shift-click the output slot
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 0,
            button: 0,
            mode: 1,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        // Ingredients should NOT be consumed (inventory was full)
        let grid = game_loop.ecs.get::<basalt_core::CraftingGrid>(eid).unwrap();
        assert_eq!(
            grid.slots[0].item_count, 2,
            "ingredients should not be consumed when inventory is full"
        );
        assert_eq!(
            grid.slots[1].item_count, 2,
            "ingredients should not be consumed when inventory is full"
        );
    }

    #[test]
    fn output_click_syncs_grid_and_cursor() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Open crafting table
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        // Place 4 oak planks in the grid and set output
        if let Some(grid) = game_loop.ecs.get_mut::<basalt_core::CraftingGrid>(eid) {
            grid.slots[0] = basalt_types::Slot::new(43, 1);
            grid.slots[1] = basalt_types::Slot::new(43, 1);
            grid.slots[3] = basalt_types::Slot::new(43, 1);
            grid.slots[4] = basalt_types::Slot::new(43, 1);
            grid.output = basalt_types::Slot::new(314, 1);
        }

        // Normal click on output — server-authoritative, changed_slots ignored
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 0,
            button: 0,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        // Server should sync consumed grid slots AND the cursor
        let mut grid_slot_syncs = 0;
        let mut cursor_syncs = 0;
        while let Ok(msg) = rx.try_recv() {
            if let ServerOutput::SetContainerSlot {
                window_id, slot, ..
            } = &msg
            {
                if (1..=9).contains(slot) {
                    grid_slot_syncs += 1;
                }
                if *window_id == -1 && *slot == -1 {
                    cursor_syncs += 1;
                }
            }
        }
        assert!(
            grid_slot_syncs > 0,
            "server should sync consumed grid slots after output click"
        );
        assert!(
            cursor_syncs > 0,
            "server should sync cursor after output click"
        );
    }

    #[test]
    fn shift_click_syncs_inventory_to_client() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Open crafting table
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        // Place 2 oak planks in each of the 4 grid slots
        if let Some(grid) = game_loop.ecs.get_mut::<basalt_core::CraftingGrid>(eid) {
            grid.slots[0] = basalt_types::Slot::new(43, 2);
            grid.slots[1] = basalt_types::Slot::new(43, 2);
            grid.slots[3] = basalt_types::Slot::new(43, 2);
            grid.slots[4] = basalt_types::Slot::new(43, 2);
            grid.output = basalt_types::Slot::new(314, 1);
        }

        // Shift-click the output slot
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 0,
            button: 0,
            mode: 1,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        // Shift-click should sync both grid AND inventory slots to client
        let mut grid_slot_syncs = 0;
        let mut inv_slot_syncs = 0;
        while let Ok(msg) = rx.try_recv() {
            if let ServerOutput::SetContainerSlot { slot, .. } = &msg {
                if (1..=9).contains(slot) {
                    grid_slot_syncs += 1;
                } else if (10..=45).contains(slot) {
                    inv_slot_syncs += 1;
                }
            }
        }
        assert!(
            grid_slot_syncs > 0,
            "shift-click should sync grid slots to client"
        );
        assert!(
            inv_slot_syncs > 0,
            "shift-click should sync inventory slots to client"
        );
    }

    #[test]
    fn left_click_grid_pick_up_from_slot() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        // Put item in grid slot, cursor empty
        if let Some(grid) = game_loop.ecs.get_mut::<basalt_core::CraftingGrid>(eid) {
            grid.slots[0] = basalt_types::Slot::new(43, 5);
        }

        // Left-click on grid slot 1 with empty cursor -> pick up
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 1,
            button: 0,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        let grid = game_loop.ecs.get::<basalt_core::CraftingGrid>(eid).unwrap();
        assert!(grid.slots[0].is_empty(), "grid slot should be empty");

        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert_eq!(inv.cursor.item_id, Some(43));
        assert_eq!(inv.cursor.item_count, 5);
    }

    #[test]
    fn left_click_grid_swap_different_items() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        // Grid has item A, cursor has item B
        if let Some(grid) = game_loop.ecs.get_mut::<basalt_core::CraftingGrid>(eid) {
            grid.slots[2] = basalt_types::Slot::new(10, 3);
        }
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .cursor = basalt_types::Slot::new(20, 7);

        // Left-click on grid slot 3 (window slot 3 = grid index 2)
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 3,
            button: 0,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        // Should swap
        let grid = game_loop.ecs.get::<basalt_core::CraftingGrid>(eid).unwrap();
        assert_eq!(grid.slots[2].item_id, Some(20));
        assert_eq!(grid.slots[2].item_count, 7);

        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert_eq!(inv.cursor.item_id, Some(10));
        assert_eq!(inv.cursor.item_count, 3);
    }

    #[test]
    fn left_click_grid_stack_same_item() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        // Grid has 30 of item, cursor has 40 -> can fit 34 more (to 64)
        if let Some(grid) = game_loop.ecs.get_mut::<basalt_core::CraftingGrid>(eid) {
            grid.slots[0] = basalt_types::Slot::new(43, 30);
        }
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .cursor = basalt_types::Slot::new(43, 40);

        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 1,
            button: 0,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        let grid = game_loop.ecs.get::<basalt_core::CraftingGrid>(eid).unwrap();
        assert_eq!(grid.slots[0].item_count, 64, "grid should be full");

        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert_eq!(inv.cursor.item_count, 6, "cursor should have remainder");
    }

    #[test]
    fn right_click_grid_pick_up_half() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        // Grid has 10 items, cursor empty
        if let Some(grid) = game_loop.ecs.get_mut::<basalt_core::CraftingGrid>(eid) {
            grid.slots[0] = basalt_types::Slot::new(43, 10);
        }

        // Right-click (button=1) on grid slot 1
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 1,
            button: 1,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        // Pick up ceil(10/2) = 5, leave 5
        let grid = game_loop.ecs.get::<basalt_core::CraftingGrid>(eid).unwrap();
        assert_eq!(grid.slots[0].item_count, 5);

        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert_eq!(inv.cursor.item_id, Some(43));
        assert_eq!(inv.cursor.item_count, 5);
    }

    #[test]
    fn right_click_grid_place_one() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        // Cursor has 5 items, grid slot empty
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .cursor = basalt_types::Slot::new(43, 5);

        // Right-click (button=1) on empty grid slot
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 1,
            button: 1,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        // Grid should have 1, cursor should have 4
        let grid = game_loop.ecs.get::<basalt_core::CraftingGrid>(eid).unwrap();
        assert_eq!(grid.slots[0].item_id, Some(43));
        assert_eq!(grid.slots[0].item_count, 1);

        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert_eq!(inv.cursor.item_count, 4);
    }

    #[test]
    fn shift_click_grid_moves_to_inventory() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        // Put item in grid slot
        if let Some(grid) = game_loop.ecs.get_mut::<basalt_core::CraftingGrid>(eid) {
            grid.slots[0] = basalt_types::Slot::new(43, 3);
        }

        // Shift-click on grid slot 1
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 1,
            button: 0,
            mode: 1,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        // Grid should be empty, inventory should have the item
        let grid = game_loop.ecs.get::<basalt_core::CraftingGrid>(eid).unwrap();
        assert!(grid.slots[0].is_empty(), "grid should be empty");

        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        let total: i32 = inv
            .slots
            .iter()
            .filter(|s| s.item_id == Some(43))
            .map(|s| s.item_count)
            .sum();
        assert_eq!(total, 3, "inventory should have 3 planks");
    }

    #[test]
    fn output_click_ignored_when_cursor_different_item() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        // Set up recipe and put different item on cursor
        if let Some(grid) = game_loop.ecs.get_mut::<basalt_core::CraftingGrid>(eid) {
            grid.slots[0] = basalt_types::Slot::new(43, 1);
            grid.slots[1] = basalt_types::Slot::new(43, 1);
            grid.slots[3] = basalt_types::Slot::new(43, 1);
            grid.slots[4] = basalt_types::Slot::new(43, 1);
            grid.output = basalt_types::Slot::new(314, 1);
        }
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .cursor = basalt_types::Slot::new(1, 10); // different item

        // Click output — should be ignored (cursor has different item)
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 0,
            button: 0,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        // Ingredients should NOT be consumed
        let grid = game_loop.ecs.get::<basalt_core::CraftingGrid>(eid).unwrap();
        assert_eq!(
            grid.slots[0].item_count, 1,
            "ingredients should not be consumed"
        );

        // Cursor should still hold the different item
        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert_eq!(inv.cursor.item_id, Some(1));
    }

    #[test]
    fn output_click_stacks_onto_cursor() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        // Set up recipe output = 4 sticks (id 331)
        if let Some(grid) = game_loop.ecs.get_mut::<basalt_core::CraftingGrid>(eid) {
            grid.slots[0] = basalt_types::Slot::new(43, 1);
            grid.slots[3] = basalt_types::Slot::new(43, 1);
            grid.output = basalt_types::Slot::new(331, 4);
        }

        // Cursor already has 2 of the same item
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .cursor = basalt_types::Slot::new(331, 2);

        // Click output — should stack
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 0,
            button: 0,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert_eq!(inv.cursor.item_id, Some(331));
        assert_eq!(inv.cursor.item_count, 6, "cursor should have 2+4=6 sticks");
    }

    #[test]
    fn drag_distributes_to_crafting_grid() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        // Set cursor to 4 items, then left-drag across 2 grid slots
        // Left drag distributes evenly: 4 / 2 = 2 each
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .cursor = basalt_types::Slot::new(43, 4);

        // StartDrag (mode=5, button=0 = left start)
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: -999,
            button: 0,
            mode: 5,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        // AddDragSlot for grid slot 0 (window slot 1)
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 1,
            button: 1,
            mode: 5,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(2);

        // AddDragSlot for grid slot 1 (window slot 2)
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 2,
            button: 1,
            mode: 5,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(3);

        // EndDrag (mode=5, button=2 = left end)
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: -999,
            button: 2,
            mode: 5,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(4);

        let grid = game_loop.ecs.get::<basalt_core::CraftingGrid>(eid).unwrap();
        assert_eq!(grid.slots[0].item_id, Some(43));
        assert_eq!(grid.slots[0].item_count, 2);
        assert_eq!(grid.slots[1].item_id, Some(43));
        assert_eq!(grid.slots[1].item_count, 2);
    }
}
