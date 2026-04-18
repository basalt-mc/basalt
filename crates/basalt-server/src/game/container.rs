//! Container views — chest opening, clicking, and viewer notifications.

use super::{GameLoop, OutputHandle};
use crate::messages::ServerOutput;

/// A part of a container backed by a block entity.
///
/// Each part maps a range of window slots to a block entity at a
/// specific position. A single chest has one part (27 slots), a
/// double chest has two (27 + 27 = 54).
#[derive(Debug, Clone)]
pub(super) struct ContainerPart {
    /// Block position of this container part.
    pub(super) position: (i32, i32, i32),
    /// First window slot index for this part.
    pub(super) slot_offset: usize,
    /// Number of slots in this part.
    pub(super) slot_count: usize,
}

/// Describes an open container window.
///
/// Abstracts single chests, double chests, and future container types.
/// The game loop builds a `ContainerView` when opening a container,
/// and uses it to route window clicks to the correct block entity.
#[derive(Debug, Clone)]
pub(super) struct ContainerView {
    /// Total number of container slots (before player inventory).
    pub(super) size: usize,
    /// The parts that compose this container.
    pub(super) parts: Vec<ContainerPart>,
    /// Minecraft window inventory type (2 = 9x3, 5 = 9x6, etc.).
    pub(super) inventory_type: i32,
    /// Window title.
    pub(super) title: String,
}

impl ContainerView {
    /// Creates a view for a single chest.
    pub(super) fn single_chest(pos: (i32, i32, i32)) -> Self {
        Self {
            size: 27,
            parts: vec![ContainerPart {
                position: pos,
                slot_offset: 0,
                slot_count: 27,
            }],
            inventory_type: 2, // generic_9x3
            title: "Chest".into(),
        }
    }

    /// Creates a view for a double chest (left half first).
    pub(super) fn double_chest(left: (i32, i32, i32), right: (i32, i32, i32)) -> Self {
        Self {
            size: 54,
            parts: vec![
                ContainerPart {
                    position: left,
                    slot_offset: 0,
                    slot_count: 27,
                },
                ContainerPart {
                    position: right,
                    slot_offset: 27,
                    slot_count: 27,
                },
            ],
            inventory_type: 5, // generic_9x6
            title: "Large Chest".into(),
        }
    }

    /// Finds which part owns a window slot and returns (position, local_index).
    pub(super) fn slot_to_part(&self, window_slot: i16) -> Option<((i32, i32, i32), usize)> {
        let ws = window_slot as usize;
        for part in &self.parts {
            if ws >= part.slot_offset && ws < part.slot_offset + part.slot_count {
                return Some((part.position, ws - part.slot_offset));
            }
        }
        None
    }

    /// Maps a window slot to a player inventory index (after container slots).
    pub(super) fn slot_to_player_inv(&self, window_slot: i16) -> Option<usize> {
        let ws = window_slot as usize;
        if ws >= self.size && ws < self.size + 27 {
            // Main inventory: internal 9-35
            Some(ws - self.size + 9)
        } else if ws >= self.size + 27 && ws < self.size + 36 {
            // Hotbar: internal 0-8
            Some(ws - self.size - 27)
        } else {
            None
        }
    }
}

impl GameLoop {
    /// Opens a chest container for a player.
    ///
    /// Creates a block entity if it doesn't exist yet, assigns a window
    /// ID, and sends OpenWindow + SetContainerContent to the client.
    pub(super) fn open_chest(&mut self, eid: basalt_ecs::EntityId, x: i32, y: i32, z: i32) {
        // Ensure block entity exists
        if self.world.get_block_entity(x, y, z).is_none() {
            self.world.set_block_entity(
                x,
                y,
                z,
                basalt_world::block_entity::BlockEntity::empty_chest(),
            );
        }
        let view = self.build_chest_view(x, y, z);
        self.open_container(eid, &view);
    }

    /// Opens a container window for a player using a generic ContainerView.
    pub(super) fn open_container(&mut self, eid: basalt_ecs::EntityId, view: &ContainerView) {
        let window_id = self.alloc_window_id();
        let mut window_slots = Vec::with_capacity(view.size + 36);

        // Container slots from block entities
        for part in &view.parts {
            let (px, py, pz) = part.position;
            if self.world.get_block_entity(px, py, pz).is_none() {
                self.world.set_block_entity(
                    px,
                    py,
                    pz,
                    basalt_world::block_entity::BlockEntity::empty_chest(),
                );
            }
            if let Some(be) = self.world.get_block_entity(px, py, pz) {
                match &*be {
                    basalt_world::block_entity::BlockEntity::Chest { slots } => {
                        window_slots.extend_from_slice(&slots[..part.slot_count.min(slots.len())]);
                    }
                }
            }
        }

        // Player inventory
        if let Some(inv) = self.ecs.get::<basalt_ecs::Inventory>(eid) {
            window_slots.extend_from_slice(&inv.slots[9..]); // main
            window_slots.extend_from_slice(&inv.slots[..9]); // hotbar
        }

        let container_pos = view.parts.first().map_or((0, 0, 0), |p| p.position);
        self.ecs.set(
            eid,
            basalt_ecs::OpenContainer {
                window_id,
                position: container_pos,
            },
        );

        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::OpenWindow {
                window_id,
                inventory_type: view.inventory_type,
                title: basalt_types::TextComponent::text(&view.title).to_nbt(),
                slots: window_slots,
            });
        });

        // Broadcast chest open animation to all players
        // Count how many players are viewing each part
        for part in &view.parts {
            let (px, py, pz) = part.position;
            let viewer_count = self
                .ecs
                .iter::<basalt_ecs::OpenContainer>()
                .filter(|(_, oc)| oc.position == container_pos)
                .count() as u8;
            for (e, _) in self.ecs.iter::<OutputHandle>() {
                self.send_to(e, |tx| {
                    let _ = tx.try_send(ServerOutput::BlockAction {
                        x: px,
                        y: py,
                        z: pz,
                        action_id: 1,
                        action_param: viewer_count.max(1),
                        block_id: 185, // chest block registry ID
                    });
                });
            }
        }
    }

    /// Builds a ContainerView for a chest at the given position.
    pub(super) fn build_chest_view(&self, x: i32, y: i32, z: i32) -> ContainerView {
        let state = self.world.get_block(x, y, z);
        let ct = basalt_world::block::chest_type(state);
        if ct == 0 {
            return ContainerView::single_chest((x, y, z));
        }
        let facing = basalt_world::block::chest_facing(state);
        let other = basalt_world::block::chest_adjacent_offsets(facing)
            .iter()
            .find_map(|&(dx, dz)| {
                let nx = x + dx;
                let nz = z + dz;
                let n = self.world.get_block(nx, y, nz);
                if basalt_world::block::is_chest(n)
                    && basalt_world::block::chest_facing(n) == facing
                    && basalt_world::block::chest_type(n) != 0
                    && basalt_world::block::chest_type(n) != ct
                {
                    Some((nx, y, nz))
                } else {
                    None
                }
            });
        match other {
            Some(other_pos) => {
                let (left, right) = if ct == 1 {
                    ((x, y, z), other_pos)
                } else {
                    (other_pos, (x, y, z))
                };
                ContainerView::double_chest(left, right)
            }
            None => ContainerView::single_chest((x, y, z)),
        }
    }

    /// Handles a WindowClick that targets an open container.
    ///
    /// Uses [`ContainerView`] to generically route slots to the correct
    /// block entity or player inventory.
    pub(super) fn handle_container_click(
        &mut self,
        eid: basalt_ecs::EntityId,
        container_pos: (i32, i32, i32),
        changed_slots: &[(i16, basalt_types::Slot)],
        cursor_item: basalt_types::Slot,
    ) {
        let view = self.build_chest_view(container_pos.0, container_pos.1, container_pos.2);

        for (window_slot, item) in changed_slots {
            if let Some((pos, local_idx)) = view.slot_to_part(*window_slot) {
                // Container slot → update block entity
                if let Some(mut be) = self.world.get_block_entity_mut(pos.0, pos.1, pos.2) {
                    match &mut *be {
                        basalt_world::block_entity::BlockEntity::Chest { slots } => {
                            if local_idx < slots.len() {
                                slots[local_idx] = item.clone();
                            }
                        }
                    }
                }
                self.world.mark_chunk_dirty(pos.0 >> 4, pos.2 >> 4);
                self.chunk_cache.invalidate(pos.0 >> 4, pos.2 >> 4);
                self.notify_container_viewers(container_pos, eid, *window_slot, item);
            } else if let Some(inv_idx) = view.slot_to_player_inv(*window_slot) {
                // Player inventory slot
                if let Some(inv) = self.ecs.get_mut::<basalt_ecs::Inventory>(eid)
                    && inv_idx < 36
                {
                    inv.slots[inv_idx] = item.clone();
                }
            }
        }

        // Update cursor
        if let Some(inv) = self.ecs.get_mut::<basalt_ecs::Inventory>(eid) {
            inv.cursor = cursor_item;
        }
    }

    /// Notifies other viewers of a container that a slot changed.
    pub(super) fn notify_container_viewers(
        &self,
        container_pos: (i32, i32, i32),
        exclude_eid: basalt_ecs::EntityId,
        window_slot: i16,
        item: &basalt_types::Slot,
    ) {
        for (other_eid, oc) in self.ecs.iter::<basalt_ecs::OpenContainer>() {
            if other_eid != exclude_eid && oc.position == container_pos {
                self.send_to(other_eid, |tx| {
                    let _ = tx.try_send(ServerOutput::SetContainerSlot {
                        window_id: oc.window_id,
                        slot: window_slot,
                        item: item.clone(),
                    });
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use basalt_types::Uuid;

    use crate::messages::{GameInput, ServerOutput};

    #[test]
    fn open_chest_sends_open_window() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        // Place chest
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CHEST);
        game_loop.world.set_block_entity(
            5,
            64,
            3,
            basalt_world::block_entity::BlockEntity::empty_chest(),
        );

        // Right-click the chest (BlockPlace on the chest block)
        let _ = game_tx.send(GameInput::BlockPlace {
            uuid,
            x: 5,
            y: 64,
            z: 3,
            direction: 1,
            sequence: 1,
        });
        game_loop.tick(1);

        let mut got_open = false;
        while let Ok(msg) = rx.try_recv() {
            if matches!(&msg, ServerOutput::OpenWindow { .. }) {
                got_open = true;
            }
        }
        assert!(got_open, "right-clicking chest should send OpenWindow");

        // Player should have OpenContainer component
        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        assert!(game_loop.ecs.has::<basalt_ecs::OpenContainer>(eid));
    }

    #[test]
    fn close_window_returns_cursor_to_inventory() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        // Put an item on the cursor
        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_ecs::Inventory>(eid)
            .unwrap()
            .cursor = basalt_types::Slot::new(1, 5);

        // Close window
        let _ = game_tx.send(GameInput::CloseWindow { uuid });
        game_loop.tick(1);

        // Cursor should be empty, item should be in inventory
        let inv = game_loop.ecs.get::<basalt_ecs::Inventory>(eid).unwrap();
        assert!(inv.cursor.is_empty(), "cursor should be empty after close");
        // Item should have been inserted somewhere
        let has_item = inv
            .slots
            .iter()
            .any(|s| s.item_id == Some(1) && s.item_count == 5);
        assert!(has_item, "cursor item should be returned to inventory");
    }

    #[test]
    fn container_click_modifies_chest() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        // Place and open chest
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CHEST);
        game_loop.world.set_block_entity(
            5,
            64,
            3,
            basalt_world::block_entity::BlockEntity::empty_chest(),
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

        // Put an item in chest slot 0 via WindowClick
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 0,
            button: 0,
            mode: 0,
            changed_slots: vec![(0, basalt_types::Slot::new(1, 10))],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(2);

        // Chest should have the item
        let be = game_loop.world.get_block_entity(5, 64, 3).unwrap();
        match &*be {
            basalt_world::block_entity::BlockEntity::Chest { slots } => {
                assert_eq!(slots[0].item_id, Some(1));
                assert_eq!(slots[0].item_count, 10);
            }
        }
    }

    #[test]
    fn container_q_drop_spawns_item() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        // Place and open chest with an item in slot 0
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CHEST);
        let mut be = basalt_world::block_entity::BlockEntity::empty_chest();
        let basalt_world::block_entity::BlockEntity::Chest { ref mut slots } = be;
        slots[0] = basalt_types::Slot::new(1, 10);
        game_loop.world.set_block_entity(5, 64, 3, be);

        // Open the chest
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

        // Q key drop from chest slot 0 (mode 4, button 0 = single)
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 0,
            button: 0,
            mode: 4,
            changed_slots: vec![(0, basalt_types::Slot::new(1, 9))],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(2);

        // Chest slot 0 should have 9 items
        let chest_be = game_loop.world.get_block_entity(5, 64, 3).unwrap();
        match &*chest_be {
            basalt_world::block_entity::BlockEntity::Chest { slots } => {
                assert_eq!(slots[0].item_count, 9);
            }
        }

        // Should have broadcast a spawn entity
        let mut got_spawn = false;
        while let Ok(msg) = rx.try_recv() {
            if matches!(&msg, ServerOutput::Broadcast(_)) {
                got_spawn = true;
            }
        }
        assert!(got_spawn, "Q drop from container should spawn item entity");
    }

    #[test]
    fn close_window_removes_open_container() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        // Manually set OpenContainer
        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        game_loop.ecs.set(
            eid,
            basalt_ecs::OpenContainer {
                window_id: 1,
                position: (5, 64, 3),
            },
        );

        let _ = game_tx.send(GameInput::CloseWindow { uuid });
        game_loop.tick(1);

        assert!(
            !game_loop.ecs.has::<basalt_ecs::OpenContainer>(eid),
            "CloseWindow should remove OpenContainer"
        );
    }

    #[test]
    fn container_drop_outside_drops_cursor() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();

        // Open a chest
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CHEST);
        game_loop.world.set_block_entity(
            5,
            64,
            3,
            basalt_world::block_entity::BlockEntity::empty_chest(),
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

        // Set cursor item
        game_loop
            .ecs
            .get_mut::<basalt_ecs::Inventory>(eid)
            .unwrap()
            .cursor = basalt_types::Slot::new(1, 8);

        // Click outside (slot -999) to drop
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: -999,
            button: 0,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(2);

        let inv = game_loop.ecs.get::<basalt_ecs::Inventory>(eid).unwrap();
        assert!(
            inv.cursor.is_empty(),
            "cursor should be empty after drop outside container"
        );
    }
}
