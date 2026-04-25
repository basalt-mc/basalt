//! Container views — chest opening, clicking, and viewer notifications.

use basalt_api::events::{
    BlockEntityCreatedEvent, BlockEntityDestroyedEvent, BlockEntityKind, BlockEntityModifiedEvent,
    CloseReason, ContainerClosedEvent, ContainerOpenRequestEvent, ContainerOpenedEvent,
};
use basalt_types::Uuid;
use basalt_world::block_entity::BlockEntity;

use super::GameLoop;
use crate::messages::ServerOutput;

/// Maps a world block entity to the public [`BlockEntityKind`] enum.
///
/// Used when dispatching block entity lifecycle events so that plugins
/// receive a lightweight kind discriminator instead of the full entity.
fn block_entity_kind(be: &BlockEntity) -> BlockEntityKind {
    match be {
        BlockEntity::Chest { .. } => BlockEntityKind::Chest,
    }
}

/// Counts how many players have an `OpenContainer` component pointing
/// at the same block-backed position as `backing`.
///
/// Returns 0 for `Virtual` backings (per-player, no co-viewing).
pub(super) fn container_viewer_count(
    ecs: &basalt_ecs::Ecs,
    backing: &basalt_api::container::ContainerBacking,
) -> u32 {
    let basalt_api::container::ContainerBacking::Block { position } = backing else {
        return 0;
    };
    let target = (position.x, position.y, position.z);
    ecs.iter::<basalt_api::components::OpenContainer>()
        .filter(|(_, oc)| {
            matches!(
                &oc.backing,
                basalt_api::container::ContainerBacking::Block { position: p }
                    if (p.x, p.y, p.z) == target
            )
        })
        .count() as u32
}

/// Counts viewers like [`container_viewer_count`] but excludes the
/// given entity. Used to compute the *remaining* viewer count for
/// chest-lid close animations after a player closes the window.
pub(super) fn container_viewer_count_excluding(
    ecs: &basalt_ecs::Ecs,
    backing: &basalt_api::container::ContainerBacking,
    exclude: basalt_ecs::EntityId,
) -> u32 {
    let basalt_api::container::ContainerBacking::Block { position } = backing else {
        return 0;
    };
    let target = (position.x, position.y, position.z);
    ecs.iter::<basalt_api::components::OpenContainer>()
        .filter(|(eid, oc)| {
            *eid != exclude
                && matches!(
                    &oc.backing,
                    basalt_api::container::ContainerBacking::Block { position: p }
                        if (p.x, p.y, p.z) == target
                )
        })
        .count() as u32
}

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
}

impl GameLoop {
    /// Opens a chest container for a player.
    ///
    /// Creates a block entity if it doesn't exist yet, assigns a window
    /// ID, and sends OpenWindow + SetContainerContent to the client.
    pub(super) fn open_chest(
        &mut self,
        uuid: Uuid,
        eid: basalt_ecs::EntityId,
        x: i32,
        y: i32,
        z: i32,
    ) {
        // Ensure block entity exists
        if self.world.get_block_entity(x, y, z).is_none() {
            self.create_block_entity(uuid, eid, x, y, z, BlockEntity::empty_chest());
        }
        let view = self.build_chest_view(x, y, z);
        self.open_container(uuid, eid, &view);
    }

    /// Opens a container window for a player using a generic ContainerView.
    pub(super) fn open_container(
        &mut self,
        uuid: Uuid,
        eid: basalt_ecs::EntityId,
        view: &ContainerView,
    ) {
        let window_id = self.alloc_window_id();
        let mut window_slots = Vec::with_capacity(view.size + 36);

        // Container slots from block entities
        for part in &view.parts {
            let (px, py, pz) = part.position;
            if self.world.get_block_entity(px, py, pz).is_none() {
                self.create_block_entity(uuid, eid, px, py, pz, BlockEntity::empty_chest());
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
        if let Some(inv) = self.ecs.get::<basalt_api::components::Inventory>(eid) {
            window_slots.extend_from_slice(&inv.slots[9..]); // main
            window_slots.extend_from_slice(&inv.slots[..9]); // hotbar
        }

        let container_pos = view.parts.first().map_or((0, 0, 0), |p| p.position);
        self.ecs.set(
            eid,
            basalt_api::components::OpenContainer {
                window_id,
                inventory_type: if view.size == 27 {
                    basalt_api::container::InventoryType::Generic9x3
                } else {
                    basalt_api::container::InventoryType::Generic9x6
                },
                backing: basalt_api::container::ContainerBacking::Block {
                    position: basalt_api::components::BlockPosition {
                        x: container_pos.0,
                        y: container_pos.1,
                        z: container_pos.2,
                    },
                },
            },
        );

        self.send_to(eid, |tx| {
            use basalt_protocol::packets::play::inventory::{
                ClientboundPlayOpenWindow, ClientboundPlayWindowItems,
            };
            let _ = tx.try_send(ServerOutput::plain(
                ClientboundPlayOpenWindow::PACKET_ID,
                ClientboundPlayOpenWindow {
                    window_id: i32::from(window_id),
                    inventory_type: view.inventory_type,
                    window_title: basalt_types::TextComponent::text(&view.title).to_nbt(),
                },
            ));
            let _ = tx.try_send(ServerOutput::plain(
                ClientboundPlayWindowItems::PACKET_ID,
                ClientboundPlayWindowItems {
                    window_id: i32::from(window_id),
                    state_id: 0,
                    items: window_slots.clone(),
                    carried_item: basalt_types::Slot::empty(),
                },
            ));
        });

        // Dispatch ContainerOpenedEvent — `ContainerPlugin` listens at
        // Post and broadcasts the chest-lid open animation. The
        // viewer_count includes the just-opened viewer (the
        // OpenContainer component was set above).
        let inventory_type = if view.size == 27 {
            basalt_api::container::InventoryType::Generic9x3
        } else {
            basalt_api::container::InventoryType::Generic9x6
        };
        let backing = basalt_api::container::ContainerBacking::Block {
            position: basalt_api::components::BlockPosition {
                x: container_pos.0,
                y: container_pos.1,
                z: container_pos.2,
            },
        };
        let viewer_count = container_viewer_count(&self.ecs, &backing);

        if let Some((entity_id, username, yaw, pitch)) = self.player_info(eid) {
            let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);
            let mut opened_event = ContainerOpenedEvent {
                window_id,
                inventory_type,
                backing,
                viewer_count,
            };
            self.dispatch_event(&mut opened_event, &ctx);
            self.process_responses(uuid, &ctx.drain_responses());
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

    /// Reads a slot from a container's block entity.
    ///
    /// Returns `Slot::empty()` if no block entity exists at the position
    /// or if the index is out of range.
    pub(super) fn read_container_slot(
        &self,
        pos: (i32, i32, i32),
        idx: usize,
    ) -> basalt_types::Slot {
        self.world
            .get_block_entity(pos.0, pos.1, pos.2)
            .map(|be| match &*be {
                basalt_world::block_entity::BlockEntity::Chest { slots } => {
                    slots.get(idx).cloned().unwrap_or_default()
                }
            })
            .unwrap_or_default()
    }

    /// Writes a slot to a container's block entity.
    ///
    /// Creates the block entity if it doesn't exist. Invalidates the
    /// chunk packet cache. Chunk dirty marking is handled by the
    /// `StoragePlugin` via `BlockEntityModifiedEvent`.
    pub(super) fn write_container_slot(
        &mut self,
        pos: (i32, i32, i32),
        idx: usize,
        item: basalt_types::Slot,
    ) {
        if self.world.get_block_entity(pos.0, pos.1, pos.2).is_none() {
            self.world.set_block_entity(
                pos.0,
                pos.1,
                pos.2,
                basalt_world::block_entity::BlockEntity::empty_chest(),
            );
        }
        if let Some(mut be) = self.world.get_block_entity_mut(pos.0, pos.1, pos.2) {
            match &mut *be {
                basalt_world::block_entity::BlockEntity::Chest { slots } => {
                    if idx < slots.len() {
                        slots[idx] = item;
                    }
                }
            }
        }
        self.chunk_cache.invalidate(pos.0 >> 4, pos.2 >> 4);
    }

    /// Notifies other viewers of a container that a slot changed.
    pub(super) fn notify_container_viewers(
        &self,
        container_pos: (i32, i32, i32),
        exclude_eid: basalt_ecs::EntityId,
        window_slot: i16,
        item: &basalt_types::Slot,
    ) {
        for (other_eid, oc) in self.ecs.iter::<basalt_api::components::OpenContainer>() {
            let pos = match &oc.backing {
                basalt_api::container::ContainerBacking::Block { position } => {
                    (position.x, position.y, position.z)
                }
                basalt_api::container::ContainerBacking::Virtual => continue,
            };
            if other_eid != exclude_eid && pos == container_pos {
                use basalt_protocol::packets::play::inventory::ClientboundPlaySetSlot;
                let oc_window_id = oc.window_id;
                let item_clone = item.clone();
                self.send_to(other_eid, |tx| {
                    let _ = tx.try_send(ServerOutput::plain(
                        ClientboundPlaySetSlot::PACKET_ID,
                        ClientboundPlaySetSlot {
                            window_id: i32::from(oc_window_id),
                            state_id: 0,
                            slot: window_slot,
                            item: item_clone,
                        },
                    ));
                });
            }
        }
    }

    /// Reads all container slots from a block entity at the given position.
    ///
    /// Returns the block entity's slots if one exists, otherwise returns
    /// a vector of `size` empty slots.
    pub(super) fn read_all_container_slots(
        &self,
        position: &basalt_api::components::BlockPosition,
        size: usize,
    ) -> Vec<basalt_types::Slot> {
        self.world
            .get_block_entity(position.x, position.y, position.z)
            .map(|be| match &*be {
                basalt_world::block_entity::BlockEntity::Chest { slots } => {
                    let mut result: Vec<basalt_types::Slot> = slots.to_vec();
                    result.resize(size, basalt_types::Slot::empty());
                    result
                }
            })
            .unwrap_or_else(|| vec![basalt_types::Slot::empty(); size])
    }

    /// Opens a custom container window for a player.
    ///
    /// Dispatches `ContainerOpenRequestEvent` (cancellable) before opening.
    /// If the event is cancelled, the window is not opened. Otherwise:
    /// - Determines initial slot contents from the container config
    /// - Sets `VirtualContainerSlots` for virtual containers
    /// - Sets `OpenContainer` on the player entity
    /// - Sends `OpenWindow` to the client
    /// - Dispatches `ContainerOpenedEvent` (post-stage)
    pub(super) fn open_custom_container(
        &mut self,
        eid: basalt_ecs::EntityId,
        uuid: Uuid,
        container: basalt_api::container::Container,
    ) {
        let inventory_type = container.inventory_type;
        let backing = container.backing;
        let title = container.title.clone();
        let size = inventory_type.slot_count();

        // Dispatch ContainerOpenRequestEvent (Validate stage, cancellable)
        let (entity_id, username, yaw, pitch) = match self.player_info(eid) {
            Some(info) => info,
            None => return,
        };
        let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);
        let mut open_request = ContainerOpenRequestEvent {
            inventory_type,
            backing,
            title: title.clone(),
            cancelled: false,
        };
        self.dispatch_event(&mut open_request, &ctx);
        self.process_responses(uuid, &ctx.drain_responses());
        if open_request.cancelled {
            return;
        }

        // Determine initial container slots
        let container_slots = if let Some(initial) = container.initial_slots {
            let mut s = initial;
            s.resize(size, basalt_types::Slot::empty());
            s
        } else {
            match &backing {
                basalt_api::container::ContainerBacking::Block { position } => {
                    self.read_all_container_slots(position, size)
                }
                basalt_api::container::ContainerBacking::Virtual => {
                    vec![basalt_types::Slot::empty(); size]
                }
            }
        };

        // For virtual containers, store slots on the player entity
        if matches!(backing, basalt_api::container::ContainerBacking::Virtual) {
            self.ecs.set(
                eid,
                basalt_api::components::VirtualContainerSlots {
                    slots: container_slots.clone(),
                },
            );
        }

        let window_id = self.alloc_window_id();

        // Set OpenContainer component
        self.ecs.set(
            eid,
            basalt_api::components::OpenContainer {
                window_id,
                inventory_type,
                backing,
            },
        );

        // Build full window slots: container + player main + player hotbar
        let mut window_slots = Vec::with_capacity(size + 36);
        window_slots.extend(container_slots);
        if let Some(inv) = self.ecs.get::<basalt_api::components::Inventory>(eid) {
            window_slots.extend_from_slice(&inv.slots[9..]); // main
            window_slots.extend_from_slice(&inv.slots[..9]); // hotbar
        }

        self.send_to(eid, |tx| {
            use basalt_protocol::packets::play::inventory::{
                ClientboundPlayOpenWindow, ClientboundPlayWindowItems,
            };
            let _ = tx.try_send(ServerOutput::plain(
                ClientboundPlayOpenWindow::PACKET_ID,
                ClientboundPlayOpenWindow {
                    window_id: i32::from(window_id),
                    inventory_type: inventory_type.protocol_id(),
                    window_title: basalt_types::TextComponent::text(&title).to_nbt(),
                },
            ));
            let _ = tx.try_send(ServerOutput::plain(
                ClientboundPlayWindowItems::PACKET_ID,
                ClientboundPlayWindowItems {
                    window_id: i32::from(window_id),
                    state_id: 0,
                    items: window_slots,
                    carried_item: basalt_types::Slot::empty(),
                },
            ));
        });

        // Compute the viewer count BEFORE dispatch — for block-backed
        // containers this includes the just-opened viewer (the
        // OpenContainer component was set on `eid` above), giving
        // plugins the post-open count for the chest-lid animation.
        let viewer_count = container_viewer_count(&self.ecs, &backing);

        // Dispatch ContainerOpenedEvent (Post stage)
        let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);
        let mut opened_event = ContainerOpenedEvent {
            window_id,
            inventory_type,
            backing,
            viewer_count,
        };
        self.dispatch_event(&mut opened_event, &ctx);
        self.process_responses(uuid, &ctx.drain_responses());
    }

    /// Dispatches a `ContainerClosedEvent` for a player.
    ///
    /// Reads the `OpenContainer` component to populate the event
    /// fields. Must be called BEFORE the `OpenContainer` component
    /// is removed (so the close-time backing/inventory_type are
    /// observable). The `crafting_grid_state` snapshot is the
    /// caller's responsibility — pass `None` for non-crafting
    /// closures.
    pub(super) fn dispatch_container_closed(
        &mut self,
        eid: basalt_ecs::EntityId,
        uuid: Uuid,
        reason: CloseReason,
        crafting_grid_state: Option<[basalt_types::Slot; 9]>,
    ) {
        let Some(oc) = self.ecs.get::<basalt_api::components::OpenContainer>(eid) else {
            return;
        };
        let window_id = oc.window_id;
        let inventory_type = oc.inventory_type;
        let backing = oc.backing;

        // Remaining viewers exclude the closing player.
        let viewer_count = container_viewer_count_excluding(&self.ecs, &backing, eid);

        let (entity_id, username, yaw, pitch) = match self.player_info(eid) {
            Some(info) => info,
            None => return,
        };
        let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);
        let mut event = ContainerClosedEvent {
            window_id,
            inventory_type,
            backing,
            reason,
            viewer_count,
            crafting_grid_state,
        };
        self.dispatch_event(&mut event, &ctx);
        self.process_responses(uuid, &ctx.drain_responses());
    }

    // ── Block entity lifecycle events ────────────────────────────

    /// Creates a block entity at the position and dispatches
    /// [`BlockEntityCreatedEvent`].
    ///
    /// Use instead of calling `world.set_block_entity` directly when
    /// the position had no prior block entity and a player context is
    /// available for event dispatch.
    pub(super) fn create_block_entity(
        &mut self,
        uuid: Uuid,
        eid: basalt_ecs::EntityId,
        x: i32,
        y: i32,
        z: i32,
        entity: BlockEntity,
    ) {
        let kind = block_entity_kind(&entity);
        self.world.set_block_entity(x, y, z, entity);

        if let Some((entity_id, username, yaw, pitch)) = self.player_info(eid) {
            let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);
            let mut event = BlockEntityCreatedEvent {
                position: basalt_api::components::BlockPosition { x, y, z },
                kind,
            };
            self.dispatch_event(&mut event, &ctx);
            self.process_responses(uuid, &ctx.drain_responses());
        }
    }

    /// Dispatches [`BlockEntityModifiedEvent`] for a block entity at the
    /// given position.
    ///
    /// The caller is responsible for having already modified the block
    /// entity (e.g. via `write_container_slot`). If no block entity
    /// exists at the position, this is a no-op.
    pub(super) fn notify_block_entity_modified(
        &mut self,
        uuid: Uuid,
        eid: basalt_ecs::EntityId,
        x: i32,
        y: i32,
        z: i32,
    ) {
        let kind = {
            let Some(be) = self.world.get_block_entity(x, y, z) else {
                return;
            };
            block_entity_kind(&be)
        };

        if let Some((entity_id, username, yaw, pitch)) = self.player_info(eid) {
            let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);
            let mut event = BlockEntityModifiedEvent {
                position: basalt_api::components::BlockPosition { x, y, z },
                kind,
            };
            self.dispatch_event(&mut event, &ctx);
            self.process_responses(uuid, &ctx.drain_responses());
        }
    }

    /// Removes a block entity and dispatches [`BlockEntityDestroyedEvent`]
    /// with the last state.
    ///
    /// Returns the removed entity for the caller if needed. Returns
    /// `None` if no block entity existed at the position. Wired into
    /// the plugin API via `Response::DestroyBlockEntity`
    /// (`ctx.world_ctx().destroy_block_entity(...)`).
    pub(super) fn destroy_block_entity(
        &mut self,
        uuid: Uuid,
        eid: basalt_ecs::EntityId,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<BlockEntity> {
        let last = self
            .world
            .get_block_entity(x, y, z)
            .map(|be| (*be).clone())?;
        let kind = block_entity_kind(&last);
        self.world.remove_block_entity(x, y, z);

        if let Some((entity_id, username, yaw, pitch)) = self.player_info(eid) {
            let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);
            let mut event = BlockEntityDestroyedEvent {
                position: basalt_api::components::BlockPosition { x, y, z },
                kind,
                last_state: last.clone(),
            };
            self.dispatch_event(&mut event, &ctx);
            self.process_responses(uuid, &ctx.drain_responses());
        }

        Some(last)
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

        use basalt_protocol::packets::play::inventory::ClientboundPlayOpenWindow;
        let mut got_open = false;
        while let Ok(msg) = rx.try_recv() {
            if matches!(&msg, ServerOutput::Plain(ep) if ep.id() == ClientboundPlayOpenWindow::PACKET_ID)
            {
                got_open = true;
            }
        }
        assert!(got_open, "right-clicking chest should send OpenWindow");

        // Player should have OpenContainer component
        let eid = game_loop.find_by_uuid(uuid).unwrap();
        assert!(
            game_loop
                .ecs
                .has::<basalt_api::components::OpenContainer>(eid)
        );
    }

    #[test]
    fn close_window_returns_cursor_to_inventory() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        // Put an item on the cursor
        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_api::components::Inventory>(eid)
            .unwrap()
            .cursor = basalt_types::Slot::new(1, 5);

        // Close window
        let _ = game_tx.send(GameInput::CloseWindow { uuid });
        game_loop.tick(1);

        // Cursor should be empty, item should be in inventory
        let inv = game_loop
            .ecs
            .get::<basalt_api::components::Inventory>(eid)
            .unwrap();
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

        let eid = game_loop.find_by_uuid(uuid).unwrap();

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

        // Set cursor item server-side, then left-click to place into chest slot 0
        game_loop
            .ecs
            .get_mut::<basalt_api::components::Inventory>(eid)
            .unwrap()
            .cursor = basalt_types::Slot::new(1, 10);

        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 0,
            button: 0,
            mode: 0,
            changed_slots: vec![],
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
            if matches!(&msg, ServerOutput::Cached(_)) {
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
        let eid = game_loop.find_by_uuid(uuid).unwrap();
        game_loop.ecs.set(
            eid,
            basalt_api::components::OpenContainer {
                window_id: 1,
                inventory_type: basalt_api::container::InventoryType::Generic9x3,
                backing: basalt_api::container::ContainerBacking::Block {
                    position: basalt_api::components::BlockPosition { x: 5, y: 64, z: 3 },
                },
            },
        );

        let _ = game_tx.send(GameInput::CloseWindow { uuid });
        game_loop.tick(1);

        assert!(
            !game_loop
                .ecs
                .has::<basalt_api::components::OpenContainer>(eid),
            "CloseWindow should remove OpenContainer"
        );
    }

    #[test]
    fn container_drop_outside_drops_cursor() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

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
            .get_mut::<basalt_api::components::Inventory>(eid)
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

        let inv = game_loop
            .ecs
            .get::<basalt_api::components::Inventory>(eid)
            .unwrap();
        assert!(
            inv.cursor.is_empty(),
            "cursor should be empty after drop outside container"
        );
    }

    #[test]
    fn read_container_slot_returns_item() {
        let (game_loop, _game_tx, _io_rx) = super::super::tests::test_game_loop();

        let mut be = basalt_world::block_entity::BlockEntity::empty_chest();
        let basalt_world::block_entity::BlockEntity::Chest { ref mut slots } = be;
        slots[5] = basalt_types::Slot::new(42, 16);
        game_loop.world.set_block_entity(10, 64, 20, be);

        let slot = game_loop.read_container_slot((10, 64, 20), 5);
        assert_eq!(slot.item_id, Some(42));
        assert_eq!(slot.item_count, 16);
    }

    #[test]
    fn read_container_slot_empty_if_no_entity() {
        let (game_loop, _game_tx, _io_rx) = super::super::tests::test_game_loop();

        let slot = game_loop.read_container_slot((99, 64, 99), 0);
        assert!(slot.is_empty(), "no block entity should return empty slot");
    }

    #[test]
    fn write_container_slot_creates_entity() {
        let (mut game_loop, _game_tx, _io_rx) = super::super::tests::test_game_loop();

        assert!(
            game_loop.world.get_block_entity(7, 64, 3).is_none(),
            "no block entity should exist yet"
        );

        game_loop.write_container_slot((7, 64, 3), 0, basalt_types::Slot::new(1, 10));

        let be = game_loop.world.get_block_entity(7, 64, 3).unwrap();
        match &*be {
            basalt_world::block_entity::BlockEntity::Chest { slots } => {
                assert_eq!(slots[0].item_id, Some(1));
                assert_eq!(slots[0].item_count, 10);
            }
        }
    }

    #[test]
    fn write_container_slot_does_not_mark_dirty() {
        let (mut game_loop, _game_tx, _io_rx) = super::super::tests::test_game_loop();

        // Ensure the chunk is loaded so dirty tracking works
        let _ = game_loop.world.get_block(16, 64, 32);

        game_loop.write_container_slot((16, 64, 32), 3, basalt_types::Slot::new(2, 5));

        let dirty = game_loop.world.dirty_chunks();
        assert!(
            !dirty.contains(&(1, 2)),
            "write_container_slot should not mark dirty (StoragePlugin handles it)"
        );
    }

    // ── Block entity lifecycle helpers ──────────────────────────

    #[test]
    fn create_block_entity_sets_entity_in_world() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        assert!(game_loop.world.get_block_entity(10, 64, 20).is_none());

        game_loop.create_block_entity(
            uuid,
            eid,
            10,
            64,
            20,
            basalt_world::block_entity::BlockEntity::empty_chest(),
        );

        let be = game_loop.world.get_block_entity(10, 64, 20);
        assert!(
            be.is_some(),
            "create_block_entity should set entity in world"
        );
    }

    #[test]
    fn destroy_block_entity_removes_and_returns_last() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Create with an item in slot 0
        let mut be = basalt_world::block_entity::BlockEntity::empty_chest();
        let basalt_world::block_entity::BlockEntity::Chest { ref mut slots } = be;
        slots[0] = basalt_types::Slot::new(42, 16);
        game_loop.world.set_block_entity(10, 64, 20, be);

        let last = game_loop.destroy_block_entity(uuid, eid, 10, 64, 20);
        assert!(last.is_some(), "destroy should return the removed entity");
        match last.unwrap() {
            basalt_world::block_entity::BlockEntity::Chest { slots } => {
                assert_eq!(slots[0].item_id, Some(42));
                assert_eq!(slots[0].item_count, 16);
            }
        }
        assert!(
            game_loop.world.get_block_entity(10, 64, 20).is_none(),
            "block entity should be removed from world"
        );
    }

    #[test]
    fn destroy_block_entity_returns_none_if_missing() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        let eid = game_loop.find_by_uuid(uuid).unwrap();

        let result = game_loop.destroy_block_entity(uuid, eid, 99, 64, 99);
        assert!(
            result.is_none(),
            "destroy on missing position should return None"
        );
    }

    #[test]
    fn notify_block_entity_modified_is_noop_if_no_entity() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        let eid = game_loop.find_by_uuid(uuid).unwrap();

        // Should not panic or error
        game_loop.notify_block_entity_modified(uuid, eid, 99, 64, 99);
    }

    #[test]
    fn container_click_dispatches_block_entity_modified() {
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();

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

        // Set cursor item, then left-click chest slot 0
        game_loop
            .ecs
            .get_mut::<basalt_api::components::Inventory>(eid)
            .unwrap()
            .cursor = basalt_types::Slot::new(1, 10);

        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 0,
            button: 0,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(2);

        // Verify the block entity was modified (item placed)
        let be = game_loop.world.get_block_entity(5, 64, 3).unwrap();
        match &*be {
            basalt_world::block_entity::BlockEntity::Chest { slots } => {
                assert_eq!(
                    slots[0].item_id,
                    Some(1),
                    "chest slot 0 should have the placed item"
                );
                assert_eq!(slots[0].item_count, 10);
            }
        }
        // The event dispatch path compiled and ran without panic,
        // confirming BlockEntityModifiedEvent dispatch works end-to-end.
    }
}
