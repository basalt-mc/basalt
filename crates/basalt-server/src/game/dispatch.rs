//! Input dispatch — drains the [`GameInput`] channel and routes messages.

use super::{ChunkStreamRate, GameLoop, Sneaking};
use crate::messages::GameInput;

impl GameLoop {
    /// Drains all pending messages from net tasks.
    pub(super) fn drain_game_input(&mut self) {
        while let Ok(msg) = self.game_rx.try_recv() {
            match msg {
                GameInput::PlayerConnected {
                    entity_id,
                    uuid,
                    username,
                    skin_properties,
                    position,
                    yaw,
                    pitch,
                    output_tx,
                } => {
                    self.handle_player_connected(
                        entity_id,
                        uuid,
                        username,
                        skin_properties,
                        position,
                        yaw,
                        pitch,
                        output_tx,
                    );
                }
                GameInput::PlayerDisconnected { uuid } => {
                    // Look up eid before disconnection removes it from the index
                    let eid = self.find_by_uuid(uuid);
                    self.handle_player_disconnected(uuid);
                    // Clean up drag state for disconnected player
                    if let Some(eid) = eid {
                        self.drag_states.remove(&eid);
                    }
                }
                GameInput::Position {
                    uuid,
                    x,
                    y,
                    z,
                    on_ground,
                } => {
                    self.handle_movement(uuid, Some((x, y, z)), None, on_ground);
                }
                GameInput::PositionLook {
                    uuid,
                    x,
                    y,
                    z,
                    yaw,
                    pitch,
                    on_ground,
                } => {
                    self.handle_movement(uuid, Some((x, y, z)), Some((yaw, pitch)), on_ground);
                }
                GameInput::Look {
                    uuid,
                    yaw,
                    pitch,
                    on_ground,
                } => {
                    self.handle_movement(uuid, None, Some((yaw, pitch)), on_ground);
                }
                GameInput::BlockDig {
                    uuid,
                    status,
                    x,
                    y,
                    z,
                    sequence,
                } => match status {
                    0 => self.handle_block_dig(uuid, x, y, z, sequence),
                    3 | 4 => self.handle_item_drop(uuid, status == 3),
                    _ => {}
                },
                GameInput::BlockPlace {
                    uuid,
                    x,
                    y,
                    z,
                    direction,
                    sequence,
                } => {
                    self.handle_block_place(uuid, x, y, z, direction, sequence);
                }
                GameInput::HeldItemSlot { uuid, slot } => {
                    if let Some(eid) = self.find_by_uuid(uuid)
                        && let Some(inv) =
                            self.ecs.get_mut::<basalt_api::components::Inventory>(eid)
                    {
                        let idx = slot as u8;
                        if idx < 9 {
                            inv.held_slot = idx;
                        }
                    }
                }
                GameInput::SetCreativeSlot { uuid, slot, item } => {
                    if slot == -1 {
                        // Creative drop: slot -1 means drop the item
                        if let Some(item_id) = item.item_id
                            && let Some(eid) = self.find_by_uuid(uuid)
                            && let Some(pos) = self.ecs.get::<basalt_api::components::Position>(eid)
                        {
                            self.spawn_item_entity(
                                pos.x as i32,
                                pos.y as i32 + 1,
                                pos.z as i32,
                                item_id,
                                item.item_count,
                            );
                        }
                    } else if let Some(eid) = self.find_by_uuid(uuid)
                        && let Some(inv) =
                            self.ecs.get_mut::<basalt_api::components::Inventory>(eid)
                        && let Some(idx) = basalt_api::components::Inventory::window_to_index(slot)
                    {
                        inv.slots[idx] = item;
                    }
                }
                GameInput::WindowClick {
                    uuid,
                    changed_slots,
                    cursor_item,
                    mode,
                    slot,
                    button,
                    ..
                } => {
                    self.handle_window_click(uuid, slot, button, mode, changed_slots, cursor_item);
                }
                GameInput::CloseWindow { uuid, .. } => {
                    if let Some(eid) = self.find_by_uuid(uuid) {
                        // Snapshot the crafting grid (if applicable) before dispatch
                        // so plugins receive the at-close state via the event.
                        let crafting_grid_state = if matches!(
                            self.ecs
                                .get::<basalt_api::components::OpenContainer>(eid)
                                .map(|oc| oc.inventory_type),
                            Some(basalt_api::container::InventoryType::Crafting)
                        ) {
                            self.ecs
                                .get::<basalt_api::components::CraftingGrid>(eid)
                                .map(|g| g.slots.clone())
                        } else {
                            None
                        };

                        // Dispatch ContainerClosedEvent before removing the component
                        self.dispatch_container_closed(
                            eid,
                            uuid,
                            basalt_api::events::CloseReason::Manual,
                            crafting_grid_state,
                        );

                        // Return cursor item to inventory or drop it
                        let cursor_item = self
                            .ecs
                            .get_mut::<basalt_api::components::Inventory>(eid)
                            .map(|inv| {
                                let item = inv.cursor.clone();
                                inv.cursor = basalt_types::Slot::empty();
                                item
                            })
                            .unwrap_or_default();
                        if let Some(item_id) = cursor_item.item_id
                            && let Some(inv) =
                                self.ecs.get_mut::<basalt_api::components::Inventory>(eid)
                            && inv.try_insert(item_id, cursor_item.item_count).is_none()
                            && let Some(pos) = self.ecs.get::<basalt_api::components::Position>(eid)
                        {
                            self.spawn_item_entity(
                                pos.x as i32,
                                pos.y as i32 + 1,
                                pos.z as i32,
                                item_id,
                                cursor_item.item_count,
                            );
                        }
                        // Read container metadata before removing the component
                        if let Some(oc) = self.ecs.get::<basalt_api::components::OpenContainer>(eid)
                        {
                            let inventory_type = oc.inventory_type;
                            let backing = oc.backing;

                            // Reset crafting grid to 2x2 after a crafting table close.
                            // Drops are handled by RecipePlugin via
                            // ContainerClosedEvent (already dispatched above with
                            // crafting_grid_state populated).
                            if matches!(
                                inventory_type,
                                basalt_api::container::InventoryType::Crafting
                            ) && let Some(grid) = self
                                .ecs
                                .get_mut::<basalt_api::components::CraftingGrid>(eid)
                            {
                                grid.grid_size = 2;
                                grid.clear();
                            }
                            // Chest close animation is now handled by ContainerPlugin
                            // listening to ContainerClosedEvent.
                            //
                            // If backing is Virtual, remove VirtualContainerSlots component
                            if matches!(backing, basalt_api::container::ContainerBacking::Virtual) {
                                self.ecs
                                    .remove_component::<basalt_api::components::VirtualContainerSlots>(eid);
                            }
                        }
                        self.ecs
                            .remove_component::<basalt_api::components::OpenContainer>(eid);
                        // Cancel any in-progress drag operation
                        self.drag_states.remove(&eid);
                    }
                }
                GameInput::EntityAction {
                    uuid, action_id, ..
                } => {
                    if let Some(eid) = self.find_by_uuid(uuid) {
                        match action_id {
                            0 => self.ecs.set(eid, Sneaking), // start sneak
                            1 => {
                                self.ecs.remove_component::<Sneaking>(eid);
                            } // stop sneak
                            _ => {}
                        }
                    }
                }
                GameInput::PlaceRecipe {
                    uuid,
                    window_id,
                    display_id,
                    make_all,
                } => {
                    self.handle_place_recipe(uuid, window_id, display_id, make_all);
                }
                GameInput::ChunkBatchAck {
                    uuid,
                    chunks_per_tick,
                } => {
                    // Reject non-finite values (NaN / ±∞) — keep the previous
                    // rate. Clamp finite values to a defensive range so a
                    // hostile or buggy client cannot push the server into
                    // sending zero chunks (stalls the player) or arbitrarily
                    // huge bursts.
                    let max_rate = self.chunk_batch_max_rate;
                    if chunks_per_tick.is_finite()
                        && let Some(eid) = self.find_by_uuid(uuid)
                        && let Some(rate) = self.ecs.get_mut::<ChunkStreamRate>(eid)
                    {
                        rate.desired_chunks_per_tick = chunks_per_tick.clamp(0.01, max_rate);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use basalt_types::Uuid;

    use crate::messages::{GameInput, ServerOutput};

    #[test]
    fn crafting_table_close_drops_grid_items() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Place a crafting table and open it
        game_loop
            .world
            .set_block(5, 64, 3, basalt_api::world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);
        while rx.try_recv().is_ok() {}

        // Put items in the crafting grid
        if let Some(grid) = game_loop
            .ecs
            .get_mut::<basalt_api::components::CraftingGrid>(eid)
        {
            grid.slots[0] = basalt_types::Slot::new(1, 4);
            grid.slots[4] = basalt_types::Slot::new(2, 8);
        }

        // Close the window
        let _ = game_tx.send(GameInput::CloseWindow { uuid });
        game_loop.tick(1);

        // Grid should be reset to 2x2 and cleared
        let grid = game_loop
            .ecs
            .get::<basalt_api::components::CraftingGrid>(eid)
            .unwrap();
        assert_eq!(grid.grid_size, 2, "grid should reset to 2x2 after close");
        for slot in &grid.slots {
            assert!(slot.is_empty(), "grid slots should be empty after close");
        }

        // Items should have been dropped (broadcast as spawn entities)
        let mut spawn_count = 0;
        while let Ok(msg) = rx.try_recv() {
            if matches!(&msg, ServerOutput::Cached(_)) {
                spawn_count += 1;
            }
        }
        assert!(
            spawn_count >= 2,
            "closing crafting table should drop grid items, got {spawn_count} broadcasts"
        );

        // OpenContainer should be removed
        assert!(
            !game_loop
                .ecs
                .has::<basalt_api::components::OpenContainer>(eid)
        );
    }

    #[test]
    fn crafting_table_close_resets_to_2x2() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Place a crafting table and open it
        game_loop
            .world
            .set_block(5, 64, 3, basalt_api::world::block::CRAFTING_TABLE);
        game_loop.open_crafting_table(eid, 5, 64, 3);

        // Verify 3x3 mode
        let grid = game_loop
            .ecs
            .get::<basalt_api::components::CraftingGrid>(eid)
            .unwrap();
        assert_eq!(grid.grid_size, 3);

        // Close
        let _ = game_tx.send(GameInput::CloseWindow { uuid });
        game_loop.tick(1);

        // Should be back to 2x2
        let grid = game_loop
            .ecs
            .get::<basalt_api::components::CraftingGrid>(eid)
            .unwrap();
        assert_eq!(grid.grid_size, 2, "should revert to 2x2 after close");
    }

    #[test]
    fn player_inventory_close_preserves_2x2_grid() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Put items in 2x2 crafting grid (no OpenContainer = player inventory)
        if let Some(grid) = game_loop
            .ecs
            .get_mut::<basalt_api::components::CraftingGrid>(eid)
        {
            grid.slots[0] = basalt_types::Slot::new(43, 1);
            grid.slots[1] = basalt_types::Slot::new(43, 1);
        }

        // Close player inventory window (no OpenContainer)
        let _ = game_tx.send(GameInput::CloseWindow { uuid });
        game_loop.tick(1);

        // 2x2 grid items should persist (no OpenContainer = not a crafting table close)
        let grid = game_loop
            .ecs
            .get::<basalt_api::components::CraftingGrid>(eid)
            .unwrap();
        assert_eq!(grid.grid_size, 2, "grid size should remain 2");
        assert_eq!(
            grid.slots[0].item_id,
            Some(43),
            "2x2 items should persist on player inventory close"
        );
    }

    #[test]
    fn drag_state_cleared_on_disconnect() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Simulate an in-progress drag operation
        game_loop.drag_states.insert(
            eid,
            super::super::click::DragState::Active {
                drag_type: 0,
                slots: vec![10, 20],
            },
        );
        assert!(game_loop.drag_states.contains_key(&eid));

        // Disconnect the player
        let _ = game_tx.send(GameInput::PlayerDisconnected { uuid });
        game_loop.tick(1);

        // Drag state should be cleaned up
        assert!(
            game_loop.drag_states.is_empty(),
            "drag state should be removed on disconnect"
        );
    }

    #[test]
    fn chunk_batch_ack_updates_per_player_rate() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Initial rate seeded from the game-loop config (25.0 in tests).
        let initial = game_loop
            .ecs
            .get::<super::super::ChunkStreamRate>(eid)
            .unwrap()
            .desired_chunks_per_tick;
        assert_eq!(initial, 25.0);

        // Healthy update — value is stored as-is (no clamp triggered).
        let _ = game_tx.send(GameInput::ChunkBatchAck {
            uuid,
            chunks_per_tick: 42.0,
        });
        game_loop.tick(1);
        assert_eq!(
            game_loop
                .ecs
                .get::<super::super::ChunkStreamRate>(eid)
                .unwrap()
                .desired_chunks_per_tick,
            42.0
        );

        // Negative / zero rates would stall the drainer — clamp to the floor.
        let _ = game_tx.send(GameInput::ChunkBatchAck {
            uuid,
            chunks_per_tick: -3.0,
        });
        game_loop.tick(2);
        assert_eq!(
            game_loop
                .ecs
                .get::<super::super::ChunkStreamRate>(eid)
                .unwrap()
                .desired_chunks_per_tick,
            0.01
        );

        // Out-of-range rates are capped at chunk_batch_max_rate (100.0 in tests).
        let _ = game_tx.send(GameInput::ChunkBatchAck {
            uuid,
            chunks_per_tick: 9_999.0,
        });
        game_loop.tick(3);
        assert_eq!(
            game_loop
                .ecs
                .get::<super::super::ChunkStreamRate>(eid)
                .unwrap()
                .desired_chunks_per_tick,
            100.0
        );

        // NaN must not corrupt the stored rate.
        let _ = game_tx.send(GameInput::ChunkBatchAck {
            uuid,
            chunks_per_tick: f32::NAN,
        });
        game_loop.tick(4);
        assert_eq!(
            game_loop
                .ecs
                .get::<super::super::ChunkStreamRate>(eid)
                .unwrap()
                .desired_chunks_per_tick,
            100.0,
            "NaN ACK must leave the previous rate untouched"
        );
    }

    #[test]
    fn drag_state_cleared_on_close_window() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Simulate an in-progress drag operation
        game_loop.drag_states.insert(
            eid,
            super::super::click::DragState::Active {
                drag_type: 1,
                slots: vec![5],
            },
        );
        assert!(game_loop.drag_states.contains_key(&eid));

        // Close the window
        let _ = game_tx.send(GameInput::CloseWindow { uuid });
        game_loop.tick(1);

        // Drag state should be cleaned up
        assert!(
            !game_loop.drag_states.contains_key(&eid),
            "drag state should be removed on close window"
        );
    }
}
