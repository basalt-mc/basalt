//! Drag-end processor for distributing items across multiple slots.

use basalt_api::events::ContainerDragEvent;
use basalt_types::Uuid;

use super::super::slots::WindowType;
use crate::game::GameLoop;
use crate::game::click::{DragState, WindowSlot};
use crate::game::click_handler;

impl GameLoop {
    /// Processes the end of a drag operation.
    ///
    /// Dispatches a [`ContainerDragEvent`] for non-player-inventory
    /// windows before applying the distribution. Returns true if any
    /// crafting grid slot was modified.
    pub(in crate::game::inventory) fn process_drag_end(
        &mut self,
        uuid: Uuid,
        eid: basalt_ecs::EntityId,
        drag_type: u8,
        wt: &WindowType,
        container_pos: Option<(i32, i32, i32)>,
    ) -> bool {
        let state = self.drag_states.remove(&eid).unwrap_or(DragState::None);
        let DragState::Active {
            drag_type: _,
            slots: drag_slots,
        } = state
        else {
            return false;
        };

        // Resolve each protocol slot to a WindowSlot
        let resolved: Vec<(i16, WindowSlot)> = drag_slots
            .iter()
            .filter_map(|&s| self.resolve_slot(wt, s).map(|ws| (s, ws)))
            .collect();
        if resolved.is_empty() {
            return false;
        }

        // Read current values of the drag target slots
        let current_values: Vec<basalt_types::Slot> = resolved
            .iter()
            .map(|(_, ws)| self.read_slot(eid, ws, container_pos))
            .collect();
        let cursor = self
            .ecs
            .get::<basalt_core::Inventory>(eid)
            .map(|inv| inv.cursor.clone())
            .unwrap_or_default();

        let is_left = drag_type == 0;
        let click_handler::DragResult {
            slots: new_values,
            cursor: new_cursor,
        } = click_handler::distribute_drag(&cursor, &current_values, is_left);

        // Dispatch ContainerDragEvent for non-player-inventory windows
        if !matches!(wt, WindowType::PlayerInventory) {
            let affected: Vec<(i16, basalt_types::Slot)> = resolved
                .iter()
                .zip(new_values.iter())
                .map(|((proto, _), val)| (*proto, val.clone()))
                .collect();
            let cancelled =
                self.dispatch_container_drag(uuid, eid, wt, affected, drag_type, &cursor);
            if cancelled {
                return false;
            }
        }

        // Write back
        let mut grid_changed = false;
        for (i, (proto_slot, ws)) in resolved.iter().enumerate() {
            let old = current_values[i].clone();
            self.write_slot(eid, ws, new_values[i].clone(), container_pos);
            self.sync_slot(eid, wt, ws, new_values[i].clone());
            if Self::is_craft_slot(ws) {
                grid_changed = true;
            }
            if let WindowSlot::Container(ci) = ws {
                if let Some(pos) = container_pos {
                    self.notify_container_viewers(pos, eid, *ci as i16, &new_values[i]);
                }
                self.dispatch_container_slot_changed(
                    uuid,
                    eid,
                    wt,
                    *proto_slot,
                    old,
                    new_values[i].clone(),
                );
            }
        }
        if let Some(inv) = self.ecs.get_mut::<basalt_core::Inventory>(eid) {
            inv.cursor = new_cursor;
        }

        grid_changed
    }

    /// Dispatches a [`ContainerDragEvent`] with the planned distribution.
    ///
    /// Returns `true` if cancelled. The `affected_slots` parameter pairs
    /// each protocol slot index with the value that will be written if the
    /// event is not cancelled.
    fn dispatch_container_drag(
        &mut self,
        uuid: Uuid,
        eid: basalt_ecs::EntityId,
        wt: &WindowType,
        affected_slots: Vec<(i16, basalt_types::Slot)>,
        drag_type: u8,
        cursor: &basalt_types::Slot,
    ) -> bool {
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
            WindowType::PlayerInventory => return false,
        };

        let api_drag_type = match drag_type {
            0 => basalt_api::events::DragType::LeftDrag,
            1 => basalt_api::events::DragType::RightDrag,
            _ => basalt_api::events::DragType::MiddleDrag,
        };

        let (entity_id, username, yaw, pitch) = match self.player_info(eid) {
            Some(info) => info,
            None => return false,
        };

        let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);
        let mut event = ContainerDragEvent {
            window_id,
            backing,
            affected_slots,
            drag_type: api_drag_type,
            cursor: cursor.clone(),
            cancelled: false,
        };
        self.dispatch_event(&mut event, &ctx);
        self.process_responses(uuid, &ctx.drain_responses());
        event.cancelled
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

    // ── Drag ───────────────────────────────────────────────────

    #[test]
    fn left_drag_distribute() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .cursor = Slot::new(1, 8);

        // Start left drag
        click(&game_tx, &mut game_loop, uuid, -999, 0, 5);
        // Add slots: main slots 9,10 (window 9,10)
        click(&game_tx, &mut game_loop, uuid, 9, 1, 5);
        click(&game_tx, &mut game_loop, uuid, 10, 1, 5);
        // End left drag
        click(&game_tx, &mut game_loop, uuid, -999, 2, 5);

        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert_eq!(inv.slots[9].item_count, 4);
        assert_eq!(inv.slots[10].item_count, 4);
        assert!(inv.cursor.is_empty());
    }

    #[test]
    fn right_drag_place_one_each() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_core::Inventory>(eid)
            .unwrap()
            .cursor = Slot::new(1, 10);

        // Start right drag
        click(&game_tx, &mut game_loop, uuid, -999, 4, 5);
        // Add 3 slots
        click(&game_tx, &mut game_loop, uuid, 9, 5, 5);
        click(&game_tx, &mut game_loop, uuid, 10, 5, 5);
        click(&game_tx, &mut game_loop, uuid, 11, 5, 5);
        // End right drag
        click(&game_tx, &mut game_loop, uuid, -999, 6, 5);

        let inv = game_loop.ecs.get::<basalt_core::Inventory>(eid).unwrap();
        assert_eq!(inv.slots[9].item_count, 1);
        assert_eq!(inv.slots[10].item_count, 1);
        assert_eq!(inv.slots[11].item_count, 1);
        assert_eq!(inv.cursor.item_count, 7);
    }
}
