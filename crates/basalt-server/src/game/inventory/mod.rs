//! Server-authoritative inventory click handling.
//!
//! The client's `changed_slots` and `cursor_item` from the WindowClick
//! packet are completely ignored. Every click result is computed from
//! the server's known state using [`ClickAction`] dispatch and the
//! pure functions in [`click_handler`].
//!
//! - [`clicks`] — individual click type processors (left, right, shift, drag, etc.)
//! - [`slots`] — slot read/write/sync helpers and window type detection

mod clicks;
mod slots;

use basalt_api::events::{ContainerClickEvent, ContainerSlotChangedEvent};
use basalt_types::Uuid;

use super::GameLoop;
use super::click::{ClickAction, DragState, click_type_from_action, parse_click_action};
use slots::WindowType;

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

        if let Some(inv) = self.ecs.get_mut::<basalt_core::Inventory>(eid) {
            if drop_count >= inv.slots[held_idx].item_count {
                inv.slots[held_idx] = basalt_types::Slot::empty();
            } else {
                inv.slots[held_idx].item_count -= drop_count;
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
            .map(|inv| inv.slots[held_idx].clone())
            .unwrap_or_default();
        self.send_to(eid, |tx| {
            let _ = tx.try_send(crate::messages::ServerOutput::SetSlot {
                slot: held_idx as i16,
                item: slot_after,
            });
        });
    }

    /// Handles a player inventory click — fully server-authoritative.
    ///
    /// The client's `changed_slots` and `cursor_item` are ignored.
    /// The click is parsed into a [`ClickAction`], the window type is
    /// determined from ECS state, and the result is computed from the
    /// server's known inventory/container/crafting state.
    #[allow(unused_variables)]
    pub(super) fn handle_window_click(
        &mut self,
        uuid: Uuid,
        slot: i16,
        button: i8,
        mode: i32,
        _changed_slots: Vec<(i16, basalt_types::Slot)>,
        _cursor_item: basalt_types::Slot,
    ) {
        let Some(eid) = self.find_by_uuid(uuid) else {
            return;
        };
        let Some(action) = parse_click_action(slot, button, mode) else {
            return;
        };
        let wt = self.determine_window_type(eid);
        let container_pos = match &wt {
            WindowType::Chest { position, .. } => Some(*position),
            _ => None,
        };

        let mut craft_grid_changed = false;

        match action {
            ClickAction::StartDrag { drag_type } => {
                self.drag_states.insert(
                    eid,
                    DragState::Active {
                        drag_type,
                        slots: Vec::new(),
                    },
                );
                return;
            }
            ClickAction::AddDragSlot { slot } => {
                if let Some(DragState::Active { slots, .. }) = self.drag_states.get_mut(&eid) {
                    slots.push(slot);
                }
                return;
            }
            ClickAction::EndDrag { drag_type } => {
                craft_grid_changed =
                    self.process_drag_end(uuid, eid, drag_type, &wt, container_pos);
            }
            ClickAction::LeftClick { slot } | ClickAction::RightClick { slot } => {
                let is_right = matches!(action, ClickAction::RightClick { .. });
                if let Some(ws) = self.resolve_slot(&wt, slot) {
                    // Dispatch ContainerClickEvent for non-player-inventory windows
                    if self.dispatch_container_click(uuid, eid, &wt, &ws, slot, &action) {
                        self.sync_cursor(eid);
                        return;
                    }
                    craft_grid_changed =
                        self.process_simple_click(uuid, eid, ws, is_right, &wt, container_pos);
                }
            }
            ClickAction::ShiftClick { slot } => {
                if let Some(ws) = self.resolve_slot(&wt, slot) {
                    if self.dispatch_container_click(uuid, eid, &wt, &ws, slot, &action) {
                        self.sync_cursor(eid);
                        return;
                    }
                    craft_grid_changed =
                        self.process_shift_click(uuid, eid, ws, &wt, container_pos);
                }
            }
            ClickAction::DropCursor { drop_all } => {
                self.process_drop_cursor(eid, drop_all);
            }
            ClickAction::DropSlot { slot, drop_all } => {
                if let Some(ws) = self.resolve_slot(&wt, slot) {
                    if self.dispatch_container_click(uuid, eid, &wt, &ws, slot, &action) {
                        self.sync_cursor(eid);
                        return;
                    }
                    self.process_drop_slot(uuid, eid, &ws, drop_all, &wt, container_pos);
                }
            }
            ClickAction::HotbarSwap { slot, hotbar } => {
                if let Some(ws) = self.resolve_slot(&wt, slot) {
                    if self.dispatch_container_click(uuid, eid, &wt, &ws, slot, &action) {
                        self.sync_cursor(eid);
                        return;
                    }
                    craft_grid_changed =
                        self.process_hotbar_swap(uuid, eid, ws, hotbar, &wt, container_pos);
                }
            }
            ClickAction::OffhandSwap { .. } => {
                // Not tracked yet — ignore
            }
            ClickAction::DoubleClick { .. } => {
                craft_grid_changed = self.process_double_click(uuid, eid, &wt, container_pos);
            }
        }

        self.sync_cursor(eid);

        if craft_grid_changed {
            self.dispatch_crafting_grid_changed(uuid, eid);
            self.update_crafting_output(eid);
        }
    }

    /// Dispatches a [`ContainerClickEvent`] for the given click.
    ///
    /// Only fires when a container is open (not for the player inventory
    /// window) and the click action maps to a public [`ContainerClickType`].
    /// Returns `true` if the event was cancelled.
    fn dispatch_container_click(
        &mut self,
        uuid: Uuid,
        eid: basalt_ecs::EntityId,
        wt: &WindowType,
        ws: &super::click::WindowSlot,
        slot_index: i16,
        action: &ClickAction,
    ) -> bool {
        // Only dispatch for open containers, not the default player inventory
        if matches!(wt, WindowType::PlayerInventory) {
            return false;
        }
        let Some(click_type) = click_type_from_action(action) else {
            return false;
        };

        let (window_id, backing) = match wt {
            WindowType::CraftingTable { window_id } => {
                let oc = self.ecs.get::<basalt_core::OpenContainer>(eid);
                let backing = oc.map_or(basalt_core::ContainerBacking::Virtual, |o| o.backing);
                (*window_id, backing)
            }
            WindowType::Chest { window_id, .. } => {
                let oc = self.ecs.get::<basalt_core::OpenContainer>(eid);
                let backing = oc.map_or(basalt_core::ContainerBacking::Virtual, |o| o.backing);
                (*window_id, backing)
            }
            WindowType::PlayerInventory => unreachable!(),
        };

        let cursor_before = self
            .ecs
            .get::<basalt_core::Inventory>(eid)
            .map(|inv| inv.cursor.clone())
            .unwrap_or_default();

        let (entity_id, username, yaw, pitch) = match self.player_info(eid) {
            Some(info) => info,
            None => return false,
        };

        let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);
        let mut event = ContainerClickEvent {
            window_id,
            backing,
            slot_index,
            window_slot_kind: ws.to_kind(),
            click_type,
            cursor_before,
            cancelled: false,
        };
        self.dispatch_event(&mut event, &ctx);
        self.process_responses(uuid, &ctx.drain_responses());
        event.cancelled
    }

    /// Dispatches a [`ContainerSlotChangedEvent`] for a container slot.
    ///
    /// Called after a container slot has been written when the old and new
    /// values differ. Only dispatches for open containers, not for the
    /// player inventory window.
    pub(in crate::game::inventory) fn dispatch_container_slot_changed(
        &mut self,
        uuid: Uuid,
        eid: basalt_ecs::EntityId,
        wt: &WindowType,
        slot_index: i16,
        old: basalt_types::Slot,
        new: basalt_types::Slot,
    ) {
        if matches!(wt, WindowType::PlayerInventory) {
            return;
        }
        if old == new {
            return;
        }

        let (window_id, backing) = match wt {
            WindowType::CraftingTable { window_id } => {
                let oc = self.ecs.get::<basalt_core::OpenContainer>(eid);
                let backing = oc.map_or(basalt_core::ContainerBacking::Virtual, |o| o.backing);
                (*window_id, backing)
            }
            WindowType::Chest { window_id, .. } => {
                let oc = self.ecs.get::<basalt_core::OpenContainer>(eid);
                let backing = oc.map_or(basalt_core::ContainerBacking::Virtual, |o| o.backing);
                (*window_id, backing)
            }
            WindowType::PlayerInventory => return,
        };

        let (entity_id, username, yaw, pitch) = match self.player_info(eid) {
            Some(info) => info,
            None => return,
        };

        let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);
        let mut event = ContainerSlotChangedEvent {
            window_id,
            backing,
            slot_index,
            old,
            new,
        };
        self.dispatch_event(&mut event, &ctx);
        self.process_responses(uuid, &ctx.drain_responses());

        // Dispatch BlockEntityModifiedEvent for block-backed containers
        if let basalt_core::ContainerBacking::Block { position } = backing {
            self.notify_block_entity_modified(uuid, eid, position.x, position.y, position.z);
        }
    }
}

#[cfg(test)]
mod tests {
    use basalt_types::{Slot, Uuid};

    use crate::messages::{GameInput, ServerOutput};

    // ── handle_item_drop ──────────────────────────────────────

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
            item: Slot::new(1, 64),
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
            item: Slot::new(1, 1),
        });
        game_loop.tick(1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert!(inv.hotbar()[0].item_id.is_none());
    }

    #[test]
    fn q_key_drop_single_item() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .slots[0] = Slot::new(1, 10);
        while rx.try_recv().is_ok() {}

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
        assert_eq!(inv.slots[0].item_count, 9);
    }

    #[test]
    fn ctrl_q_drop_full_stack() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .slots[0] = Slot::new(1, 32);

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
        assert!(inv.slots[0].is_empty());
    }

    #[test]
    fn creative_drop_slot_minus_one() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let _ = game_tx.send(GameInput::SetCreativeSlot {
            uuid,
            slot: -1,
            item: Slot::new(1, 5),
        });
        game_loop.tick(1);

        let mut got_spawn = false;
        while let Ok(msg) = rx.try_recv() {
            if matches!(&msg, ServerOutput::Broadcast(_)) {
                got_spawn = true;
            }
        }
        assert!(got_spawn, "creative drop should spawn item entity");
    }
}
