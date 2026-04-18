//! Input dispatch — drains the [`GameInput`] channel and routes messages.

use super::{GameLoop, Sneaking};
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
                    self.handle_player_disconnected(uuid);
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
                    if let Some(eid) = self.ecs.find_by_uuid(uuid)
                        && let Some(inv) = self.ecs.get_mut::<basalt_ecs::Inventory>(eid)
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
                            && let Some(eid) = self.ecs.find_by_uuid(uuid)
                            && let Some(pos) = self.ecs.get::<basalt_ecs::Position>(eid)
                        {
                            self.spawn_item_entity(
                                pos.x as i32,
                                pos.y as i32 + 1,
                                pos.z as i32,
                                item_id,
                                item.item_count,
                            );
                        }
                    } else if let Some(eid) = self.ecs.find_by_uuid(uuid)
                        && let Some(inv) = self.ecs.get_mut::<basalt_ecs::Inventory>(eid)
                        && let Some(idx) = basalt_ecs::Inventory::window_to_index(slot)
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
                    if let Some(eid) = self.ecs.find_by_uuid(uuid) {
                        // Return cursor item to inventory or drop it
                        let cursor_item = self
                            .ecs
                            .get_mut::<basalt_ecs::Inventory>(eid)
                            .map(|inv| {
                                let item = inv.cursor.clone();
                                inv.cursor = basalt_types::Slot::empty();
                                item
                            })
                            .unwrap_or_default();
                        if let Some(item_id) = cursor_item.item_id
                            && let Some(inv) = self.ecs.get_mut::<basalt_ecs::Inventory>(eid)
                            && inv.try_insert(item_id, cursor_item.item_count).is_none()
                            && let Some(pos) = self.ecs.get::<basalt_ecs::Position>(eid)
                        {
                            self.spawn_item_entity(
                                pos.x as i32,
                                pos.y as i32 + 1,
                                pos.z as i32,
                                item_id,
                                cursor_item.item_count,
                            );
                        }
                        // Broadcast chest close animation if no other viewers
                        if let Some(oc) = self.ecs.get::<basalt_ecs::OpenContainer>(eid) {
                            let pos = oc.position;
                            let remaining = self
                                .ecs
                                .iter::<basalt_ecs::OpenContainer>()
                                .filter(|(id, oc2)| *id != eid && oc2.position == pos)
                                .count() as u8;
                            let view = self.build_chest_view(pos.0, pos.1, pos.2);
                            for part in &view.parts {
                                let (px, py, pz) = part.position;
                                for (e, _) in self.ecs.iter::<super::OutputHandle>() {
                                    self.send_to(e, |tx| {
                                        let _ = tx.try_send(
                                            crate::messages::ServerOutput::BlockAction {
                                                x: px,
                                                y: py,
                                                z: pz,
                                                action_id: 1,
                                                action_param: remaining,
                                                block_id: 185,
                                            },
                                        );
                                    });
                                }
                            }
                        }
                        self.ecs.remove_component::<basalt_ecs::OpenContainer>(eid);
                    }
                }
                GameInput::EntityAction {
                    uuid, action_id, ..
                } => {
                    if let Some(eid) = self.ecs.find_by_uuid(uuid) {
                        match action_id {
                            0 => self.ecs.set(eid, Sneaking), // start sneak
                            1 => {
                                self.ecs.remove_component::<Sneaking>(eid);
                            } // stop sneak
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}
