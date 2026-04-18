//! Game loop — single dedicated OS thread for all tick-based simulation.
//!
//! Runs at 20 TPS. Each tick:
//! 1. Drains the [`GameInput`] channel (connect, disconnect, movement, blocks, inventory)
//! 2. Runs ECS systems (physics, AI, pathfinding)
//! 3. Produces [`ServerOutput`] game events to player net tasks (zero encoding)

use std::collections::HashSet;
use std::sync::Arc;

use basalt_api::EventBus;
use basalt_api::context::{Response, ServerContext};
use basalt_api::events::{
    BlockBrokenEvent, BlockPlacedEvent, PlayerJoinedEvent, PlayerLeftEvent, PlayerMovedEvent,
};
use basalt_events::Event;
use basalt_protocol::packets::play::chat::ClientboundPlayDeclareCommands;
use basalt_protocol::packets::play::entity::ClientboundPlaySpawnEntity;
use basalt_protocol::packets::play::player::{ClientboundPlayLogin, ClientboundPlayLoginSpawninfo};
use basalt_types::{Encode, Position, Uuid, VarInt, Vec3i16};
use tokio::sync::mpsc;

use crate::helpers::angle_to_byte;
use crate::messages::{BroadcastEvent, EncodablePacket, GameInput, ServerOutput, SharedBroadcast};
use crate::net::chunk_cache::ChunkPacketCache;

/// View distance radius in chunks.
const VIEW_RADIUS: i32 = 5;

/// Channel handle for sending output packets to a player's net task.
///
/// Server-internal component (depends on tokio, not in basalt-ecs).
struct OutputHandle {
    /// Sender for the player's output channel.
    tx: mpsc::Sender<ServerOutput>,
}
impl basalt_ecs::Component for OutputHandle {}

/// Whether a player is currently sneaking (shift key held).
///
/// Affects block interaction: sneaking players place blocks instead
/// of opening containers.
struct Sneaking;
impl basalt_ecs::Component for Sneaking {}

/// A part of a container backed by a block entity.
///
/// Each part maps a range of window slots to a block entity at a
/// specific position. A single chest has one part (27 slots), a
/// double chest has two (27 + 27 = 54).
#[derive(Debug, Clone)]
struct ContainerPart {
    /// Block position of this container part.
    position: (i32, i32, i32),
    /// First window slot index for this part.
    slot_offset: usize,
    /// Number of slots in this part.
    slot_count: usize,
}

/// Describes an open container window.
///
/// Abstracts single chests, double chests, and future container types.
/// The game loop builds a `ContainerView` when opening a container,
/// and uses it to route window clicks to the correct block entity.
#[derive(Debug, Clone)]
struct ContainerView {
    /// Total number of container slots (before player inventory).
    size: usize,
    /// The parts that compose this container.
    parts: Vec<ContainerPart>,
    /// Minecraft window inventory type (2 = 9x3, 5 = 9x6, etc.).
    inventory_type: i32,
    /// Window title.
    title: String,
}

impl ContainerView {
    /// Creates a view for a single chest.
    fn single_chest(pos: (i32, i32, i32)) -> Self {
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
    fn double_chest(left: (i32, i32, i32), right: (i32, i32, i32)) -> Self {
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
    fn slot_to_part(&self, window_slot: i16) -> Option<((i32, i32, i32), usize)> {
        let ws = window_slot as usize;
        for part in &self.parts {
            if ws >= part.slot_offset && ws < part.slot_offset + part.slot_count {
                return Some((part.position, ws - part.slot_offset));
            }
        }
        None
    }

    /// Maps a window slot to a player inventory index (after container slots).
    fn slot_to_player_inv(&self, window_slot: i16) -> Option<usize> {
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

/// Mojang skin texture data.
///
/// Server-internal component storing skin properties for broadcasting
/// to other players on join.
struct SkinData {
    /// Mojang profile properties (name, value, signature).
    properties: Vec<basalt_core::broadcast::ProfileProperty>,
}
impl basalt_ecs::Component for SkinData {}

/// Tracks which chunks a player's client currently has loaded.
///
/// Server-internal component for delta-based chunk streaming.
struct ChunkView {
    /// Set of chunk coordinates loaded by the client.
    loaded_chunks: std::collections::HashSet<(i32, i32)>,
}

impl ChunkView {
    /// Creates an empty chunk view.
    fn empty() -> Self {
        Self {
            loaded_chunks: std::collections::HashSet::new(),
        }
    }
}
impl basalt_ecs::Component for ChunkView {}

/// The game loop state and logic.
pub(crate) struct GameLoop {
    /// Game event bus (blocks, movement, lifecycle).
    bus: EventBus,
    /// World — sole owner for writes, concurrent reads by net tasks.
    world: Arc<basalt_world::World>,
    /// Shared chunk packet cache — invalidated on block mutations.
    chunk_cache: Arc<ChunkPacketCache>,
    /// Entity Component System: entities, components, systems.
    ecs: basalt_ecs::Ecs,
    /// Receiver for net task → game loop messages.
    game_rx: mpsc::UnboundedReceiver<GameInput>,
    /// Sender for the I/O thread (async chunk persistence).
    io_tx: mpsc::UnboundedSender<crate::runtime::io_thread::IoRequest>,
    /// Counter for assigning entity IDs to non-player entities (items, mobs).
    next_entity_id: std::sync::Arc<std::sync::atomic::AtomicI32>,
    /// Pre-built DeclareCommands packet payload.
    declare_commands: Vec<u8>,
    /// Chunks within simulation distance of any player.
    ///
    /// Updated when players connect, disconnect, or cross chunk boundaries.
    /// ECS systems should only process entities in active chunks.
    active_chunks: HashSet<(i32, i32)>,
    /// Simulation distance in chunks around each player.
    simulation_distance: i32,
    /// How often to flush dirty chunks to disk, in ticks.
    persistence_interval_ticks: u64,
    /// Cycling counter for container window IDs (1-127).
    next_window_id: u8,
}

impl GameLoop {
    /// Creates a new game loop.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bus: EventBus,
        world: Arc<basalt_world::World>,
        chunk_cache: Arc<ChunkPacketCache>,
        game_rx: mpsc::UnboundedReceiver<GameInput>,
        io_tx: mpsc::UnboundedSender<crate::runtime::io_thread::IoRequest>,
        ecs: basalt_ecs::Ecs,
        declare_commands: Vec<u8>,
        next_entity_id: std::sync::Arc<std::sync::atomic::AtomicI32>,
        simulation_distance: i32,
        persistence_interval_ticks: u64,
    ) -> Self {
        Self {
            bus,
            world,
            chunk_cache,
            ecs,
            game_rx,
            io_tx,
            next_entity_id,
            declare_commands,
            active_chunks: HashSet::new(),
            simulation_distance,
            persistence_interval_ticks,
            next_window_id: 1,
        }
    }

    /// Allocates the next container window ID (1-127, cycling).
    fn alloc_window_id(&mut self) -> u8 {
        let id = self.next_window_id;
        self.next_window_id = if id >= 127 { 1 } else { id + 1 };
        id
    }

    /// Processes one tick.
    pub fn tick(&mut self, tick: u64) {
        self.drain_game_input();
        self.ecs.run_all(tick);
        self.broadcast_item_movement();
        self.tick_item_pickup();
        self.collect_expired_entities();
        self.flush_dirty_chunks_if_due(tick);
    }

    /// Broadcasts position updates for non-player entities that have velocity.
    ///
    /// After physics runs (via ecs.run_all), entities may have moved.
    /// Players get movement broadcast via handle_movement, but non-player
    /// entities (dropped items) need separate broadcasts.
    fn broadcast_item_movement(&mut self) {
        let moving: Vec<(basalt_ecs::EntityId, f64, f64, f64)> = self
            .ecs
            .iter::<basalt_ecs::DroppedItem>()
            .filter_map(|(eid, _)| {
                let vel = self.ecs.get::<basalt_ecs::Velocity>(eid)?;
                // Only broadcast if actually moving
                if vel.dx.abs() < 0.001 && vel.dy.abs() < 0.001 && vel.dz.abs() < 0.001 {
                    return None;
                }
                let pos = self.ecs.get::<basalt_ecs::Position>(eid)?;
                Some((eid, pos.x, pos.y, pos.z))
            })
            .collect();

        if moving.is_empty() {
            return;
        }

        for (eid, x, y, z) in moving {
            let bc = Arc::new(SharedBroadcast::new(BroadcastEvent::EntityMoved {
                entity_id: eid as i32,
                x,
                y,
                z,
                yaw: 0.0,
                pitch: 0.0,
                on_ground: false,
            }));
            for (player_eid, _) in self.ecs.iter::<OutputHandle>() {
                self.send_to(player_eid, |tx| {
                    let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&bc)));
                });
            }
        }
    }

    /// Checks proximity between item entities and players, picks up items.
    ///
    /// For each dropped item with an expired pickup delay, checks distance
    /// to all players. If within 1.5 blocks and the player's inventory has
    /// space, the item is transferred, a collect animation is broadcast,
    /// and the item entity is despawned.
    fn tick_item_pickup(&mut self) {
        // Collect all item entities eligible for pickup
        let items: Vec<(basalt_ecs::EntityId, f64, f64, f64, i32, i32)> = self
            .ecs
            .iter::<basalt_ecs::DroppedItem>()
            .filter_map(|(eid, item)| {
                // Skip items still on pickup delay
                if let Some(delay) = self.ecs.get::<basalt_ecs::PickupDelay>(eid)
                    && delay.remaining_ticks > 0
                {
                    return None;
                }
                let pos = self.ecs.get::<basalt_ecs::Position>(eid)?;
                let item_id = item.slot.item_id?;
                Some((eid, pos.x, pos.y, pos.z, item_id, item.slot.item_count))
            })
            .collect();

        if items.is_empty() {
            return;
        }

        // Collect all players
        let players: Vec<(basalt_ecs::EntityId, f64, f64, f64)> = self
            .ecs
            .iter::<basalt_ecs::PlayerRef>()
            .filter_map(|(eid, _)| {
                let pos = self.ecs.get::<basalt_ecs::Position>(eid)?;
                Some((eid, pos.x, pos.y, pos.z))
            })
            .collect();

        const PICKUP_RADIUS_SQ: f64 = 1.5 * 1.5;

        for (item_eid, ix, iy, iz, item_id, count) in &items {
            for (player_eid, px, py, pz) in &players {
                let dx = ix - px;
                let dy = iy - py;
                let dz = iz - pz;
                let dist_sq = dx * dx + dy * dy + dz * dz;

                if dist_sq > PICKUP_RADIUS_SQ {
                    continue;
                }

                // Try to insert into player inventory
                let (inv_idx, slot_after) = {
                    let Some(inv) = self.ecs.get_mut::<basalt_ecs::Inventory>(*player_eid) else {
                        continue;
                    };
                    let Some(idx) = inv.try_insert(*item_id, *count) else {
                        continue;
                    };
                    (idx, inv.slots[idx].clone())
                };

                // Send SetSlot to sync (raw internal index = SetPlayerInventory slot)
                self.send_to(*player_eid, |tx| {
                    let _ = tx.try_send(ServerOutput::SetSlot {
                        slot: inv_idx as i16,
                        item: slot_after,
                    });
                });

                // Broadcast collect animation + entity destroy
                let collect = Arc::new(SharedBroadcast::new(BroadcastEvent::CollectItem {
                    collected_entity_id: *item_eid as i32,
                    collector_entity_id: *player_eid as i32,
                    count: *count,
                }));
                let destroy = Arc::new(SharedBroadcast::new(BroadcastEvent::RemoveEntities {
                    entity_ids: vec![*item_eid as i32],
                }));
                for (e, _) in self.ecs.iter::<OutputHandle>() {
                    self.send_to(e, |tx| {
                        let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&collect)));
                        let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&destroy)));
                    });
                }

                // Despawn the item entity
                self.ecs.despawn(*item_eid);
                break; // item is consumed, move to next item
            }
        }
    }

    /// Despawns entities whose lifetime has expired.
    ///
    /// The lifetime decrement is handled by the `lifetime_system` ECS
    /// system (registered in lib.rs). This method collects entities
    /// that reached zero and handles the side effects (broadcast + despawn).
    fn collect_expired_entities(&mut self) {
        let mut expired = Vec::new();
        for (eid, lt) in self.ecs.iter::<basalt_ecs::Lifetime>() {
            if lt.remaining_ticks == 0 {
                expired.push(eid);
            }
        }

        if expired.is_empty() {
            return;
        }

        let entity_ids: Vec<i32> = expired.iter().map(|&eid| eid as i32).collect();
        let bc = Arc::new(SharedBroadcast::new(BroadcastEvent::RemoveEntities {
            entity_ids,
        }));
        for (player_eid, _) in self.ecs.iter::<OutputHandle>() {
            self.send_to(player_eid, |tx| {
                let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&bc)));
            });
        }
        for eid in expired {
            self.ecs.despawn(eid);
        }
    }

    /// Periodically flushes all dirty chunks to the I/O thread.
    ///
    /// Runs every `persistence_interval_ticks` ticks (~30s at 20 TPS).
    /// Collects dirty chunks from the World and sends them as batch
    /// persist requests to the I/O thread.
    fn flush_dirty_chunks_if_due(&self, tick: u64) {
        if self.persistence_interval_ticks == 0
            || !tick.is_multiple_of(self.persistence_interval_ticks)
        {
            return;
        }
        let dirty = self.world.dirty_chunks();
        if dirty.is_empty() {
            return;
        }
        log::debug!(target: "basalt::game", "Flushing {} dirty chunks to disk", dirty.len());
        for (cx, cz) in dirty {
            let _ = self
                .io_tx
                .send(crate::runtime::io_thread::IoRequest::PersistChunk { cx, cz });
        }
    }

    /// Recalculates the set of active chunks from all player positions.
    ///
    /// A chunk is active if it falls within `simulation_distance` of
    /// any connected player.
    fn rebuild_active_chunks(&mut self) {
        self.active_chunks.clear();
        let sd = self.simulation_distance;
        for (_, pos) in self.ecs.iter::<basalt_ecs::Position>() {
            // Only count entities that are players
            let cx = (pos.x as i32) >> 4;
            let cz = (pos.z as i32) >> 4;
            for dx in -sd..=sd {
                for dz in -sd..=sd {
                    self.active_chunks.insert((cx + dx, cz + dz));
                }
            }
        }
    }

    /// Returns whether the chunk at (cx, cz) is within simulation distance.
    #[allow(dead_code)]
    pub fn is_chunk_active(&self, cx: i32, cz: i32) -> bool {
        self.active_chunks.contains(&(cx, cz))
    }

    /// Drains all pending messages from net tasks.
    fn drain_game_input(&mut self) {
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
                                for (e, _) in self.ecs.iter::<OutputHandle>() {
                                    self.send_to(e, |tx| {
                                        let _ = tx.try_send(ServerOutput::BlockAction {
                                            x: px,
                                            y: py,
                                            z: pz,
                                            action_id: 1,
                                            action_param: remaining,
                                            block_id: 185,
                                        });
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

    /// Handles a player inventory click.
    ///
    /// The Minecraft client sends the expected result of the click in
    /// `changed_slots`. We trust the client's calculation and apply it
    /// Handles dropping the held item via BlockDig status 3/4 (Q key).
    ///
    /// Status 3 = drop entire stack, status 4 = drop single item.
    fn handle_item_drop(&mut self, uuid: Uuid, drop_stack: bool) {
        let Some(eid) = self.ecs.find_by_uuid(uuid) else {
            return;
        };
        let (item_id, drop_count, held_idx) = {
            let Some(inv) = self.ecs.get::<basalt_ecs::Inventory>(eid) else {
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

        // Decrement or clear the slot
        if let Some(inv) = self.ecs.get_mut::<basalt_ecs::Inventory>(eid) {
            if drop_count >= inv.slots[held_idx].item_count {
                inv.slots[held_idx] = basalt_types::Slot::empty();
            } else {
                inv.slots[held_idx].item_count -= drop_count;
            }
        }

        // Spawn the dropped item entity
        if let Some(pos) = self.ecs.get::<basalt_ecs::Position>(eid) {
            self.spawn_item_entity(
                pos.x as i32,
                pos.y as i32 + 1,
                pos.z as i32,
                item_id,
                drop_count,
            );
        }

        // Sync the changed slot (raw internal index = SetPlayerInventory slot)
        let slot_after = self
            .ecs
            .get::<basalt_ecs::Inventory>(eid)
            .map(|inv| inv.slots[held_idx].clone())
            .unwrap_or_default();
        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::SetSlot {
                slot: held_idx as i16,
                item: slot_after,
            });
        });
    }

    /// Handles a player inventory click.
    ///
    /// Handles a player inventory click.
    ///
    /// The client sends the expected result in `changed_slots` and
    /// `cursor_item`. We apply them server-side and handle drops.
    ///
    /// Key flows:
    /// - Click outside (slot -999): drop the OLD cursor item
    /// - Mode 4 (Q key in inventory): drop from hovered slot
    /// - All others: apply changed_slots + update cursor
    fn handle_window_click(
        &mut self,
        uuid: Uuid,
        slot: i16,
        button: i8,
        mode: i32,
        changed_slots: Vec<(i16, basalt_types::Slot)>,
        cursor_item: basalt_types::Slot,
    ) {
        let Some(eid) = self.ecs.find_by_uuid(uuid) else {
            return;
        };

        // If a container is open, route to container click handler
        if let Some(oc) = self.ecs.get::<basalt_ecs::OpenContainer>(eid) {
            let pos = oc.position;
            // Drop outside container window
            if slot == -999 {
                let old_cursor = self
                    .ecs
                    .get::<basalt_ecs::Inventory>(eid)
                    .map(|inv| inv.cursor.clone())
                    .unwrap_or_default();
                if let Some(item_id) = old_cursor.item_id
                    && let Some(player_pos) = self.ecs.get::<basalt_ecs::Position>(eid)
                {
                    self.spawn_item_entity(
                        player_pos.x as i32,
                        player_pos.y as i32 + 1,
                        player_pos.z as i32,
                        item_id,
                        old_cursor.item_count,
                    );
                }
                if let Some(inv) = self.ecs.get_mut::<basalt_ecs::Inventory>(eid) {
                    inv.cursor = cursor_item;
                }
                return;
            }
            // Mode 4: Q key drop while hovering a container slot
            if mode == 4 && slot >= 0 {
                // Determine what to drop: container slot or player slot
                let ws = slot;
                let drop_item = if (0..27).contains(&ws) {
                    // Chest slot
                    self.world
                        .get_block_entity(pos.0, pos.1, pos.2)
                        .map(|be| match &*be {
                            basalt_world::block_entity::BlockEntity::Chest { slots } => {
                                slots[ws as usize].clone()
                            }
                        })
                } else if (27..54).contains(&ws) {
                    let idx = (ws - 27 + 9) as usize;
                    self.ecs
                        .get::<basalt_ecs::Inventory>(eid)
                        .and_then(|inv| (idx < 36).then(|| inv.slots[idx].clone()))
                } else if (54..63).contains(&ws) {
                    let idx = (ws - 54) as usize;
                    self.ecs
                        .get::<basalt_ecs::Inventory>(eid)
                        .map(|inv| inv.slots[idx].clone())
                } else {
                    None
                };

                if let Some(item) = drop_item
                    && let Some(item_id) = item.item_id
                {
                    let drop_count = if button == 0 { 1 } else { item.item_count };
                    // Apply the changed_slots from the client (handles decrement)
                    self.handle_container_click(eid, pos, &changed_slots, cursor_item);
                    // Spawn the dropped item
                    if let Some(player_pos) = self.ecs.get::<basalt_ecs::Position>(eid) {
                        self.spawn_item_entity(
                            player_pos.x as i32,
                            player_pos.y as i32 + 1,
                            player_pos.z as i32,
                            item_id,
                            drop_count,
                        );
                    }
                }
                return;
            }

            self.handle_container_click(eid, pos, &changed_slots, cursor_item);
            return;
        }

        // Click outside window (slot -999): drop what was on the cursor
        if slot == -999 {
            let old_cursor = {
                let Some(inv) = self.ecs.get::<basalt_ecs::Inventory>(eid) else {
                    return;
                };
                inv.cursor.clone()
            };
            if let Some(item_id) = old_cursor.item_id
                && let Some(pos) = self.ecs.get::<basalt_ecs::Position>(eid)
            {
                self.spawn_item_entity(
                    pos.x as i32,
                    pos.y as i32 + 1,
                    pos.z as i32,
                    item_id,
                    old_cursor.item_count,
                );
            }
            // Update cursor (now empty) and apply any changed_slots
            if let Some(inv) = self.ecs.get_mut::<basalt_ecs::Inventory>(eid) {
                inv.cursor = cursor_item;
                for (window_slot, item) in &changed_slots {
                    if let Some(idx) = basalt_ecs::Inventory::window_to_index(*window_slot) {
                        inv.slots[idx] = item.clone();
                    }
                }
            }
            return;
        }

        // Mode 4: Q key while hovering a slot in open inventory
        if mode == 4 && slot >= 0 {
            if let Some(idx) = basalt_ecs::Inventory::window_to_index(slot) {
                let item = {
                    let Some(inv) = self.ecs.get::<basalt_ecs::Inventory>(eid) else {
                        return;
                    };
                    inv.slots[idx].clone()
                };
                if let Some(item_id) = item.item_id {
                    let drop_count = if button == 0 { 1 } else { item.item_count };
                    if let Some(inv) = self.ecs.get_mut::<basalt_ecs::Inventory>(eid) {
                        if drop_count >= inv.slots[idx].item_count {
                            inv.slots[idx] = basalt_types::Slot::empty();
                        } else {
                            inv.slots[idx].item_count -= drop_count;
                        }
                    }
                    if let Some(pos) = self.ecs.get::<basalt_ecs::Position>(eid) {
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
                        .get::<basalt_ecs::Inventory>(eid)
                        .map(|inv| inv.slots[idx].clone())
                        .unwrap_or_default();
                    self.send_to(eid, |tx| {
                        let _ = tx.try_send(ServerOutput::SetSlot {
                            slot: idx as i16,
                            item: slot_after,
                        });
                    });
                }
            }
            return;
        }

        // All other clicks: apply changed_slots + update cursor
        if let Some(inv) = self.ecs.get_mut::<basalt_ecs::Inventory>(eid) {
            for (window_slot, item) in &changed_slots {
                if let Some(idx) = basalt_ecs::Inventory::window_to_index(*window_slot) {
                    inv.slots[idx] = item.clone();
                }
            }
            inv.cursor = cursor_item;
        }
    }

    // ── Player lifecycle ──────────────────────────────────────────────

    /// Handles a new player connection: spawn entity, send initial world, broadcast join.
    #[allow(clippy::too_many_arguments)]
    fn handle_player_connected(
        &mut self,
        entity_id: i32,
        uuid: Uuid,
        username: String,
        skin_properties: Vec<basalt_core::broadcast::ProfileProperty>,
        position: (f64, f64, f64),
        yaw: f32,
        pitch: f32,
        output_tx: mpsc::Sender<ServerOutput>,
    ) {
        let eid = entity_id as basalt_ecs::EntityId;
        self.ecs.spawn_with_id(eid);
        self.ecs.set(
            eid,
            basalt_ecs::PlayerRef {
                uuid,
                username: username.clone(),
            },
        );
        self.ecs.set(
            eid,
            basalt_ecs::Position {
                x: position.0,
                y: position.1,
                z: position.2,
            },
        );
        self.ecs.set(eid, basalt_ecs::Rotation { yaw, pitch });
        self.ecs.set(
            eid,
            basalt_ecs::BoundingBox {
                width: 0.6,
                height: 1.8,
            },
        );
        self.ecs.set(eid, basalt_ecs::Inventory::empty());
        self.ecs.set(
            eid,
            SkinData {
                properties: skin_properties.clone(),
            },
        );
        self.ecs.set(eid, ChunkView::empty());
        self.ecs.set(eid, OutputHandle { tx: output_tx });
        self.ecs.index_uuid(uuid, eid);

        // Send initial world data
        self.send_initial_world(eid, entity_id, position);

        // Send existing players to the new player + broadcast join
        let snapshot = basalt_api::PlayerSnapshot {
            username: username.clone(),
            uuid,
            entity_id,
            x: position.0,
            y: position.1,
            z: position.2,
            yaw,
            pitch,
            skin_properties,
        };

        // Send self info to new player
        self.send_to(eid, |tx| send_player_info_add(tx, &snapshot));

        // Send all existing players to the new player, and broadcast join to them
        let other_eids: Vec<basalt_ecs::EntityId> = self
            .ecs
            .iter::<basalt_ecs::PlayerRef>()
            .filter(|&(id, _)| id != eid)
            .map(|(id, _)| id)
            .collect();

        for other_eid in &other_eids {
            // Build snapshot of existing player
            if let (Some(pr), Some(pos), Some(rot)) = (
                self.ecs.get::<basalt_ecs::PlayerRef>(*other_eid),
                self.ecs.get::<basalt_ecs::Position>(*other_eid),
                self.ecs.get::<basalt_ecs::Rotation>(*other_eid),
            ) {
                let skin = self
                    .ecs
                    .get::<SkinData>(*other_eid)
                    .map(|s| s.properties.clone())
                    .unwrap_or_default();
                let other_snapshot = basalt_api::PlayerSnapshot {
                    username: pr.username.clone(),
                    uuid: pr.uuid,
                    entity_id: *other_eid as i32,
                    x: pos.x,
                    y: pos.y,
                    z: pos.z,
                    yaw: rot.yaw,
                    pitch: rot.pitch,
                    skin_properties: skin,
                };
                // Send existing player info to new player
                self.send_to(eid, |tx| send_player_info_add(tx, &other_snapshot));
                self.send_to(eid, |tx| send_spawn_entity(tx, &other_snapshot));
            }

            // Send new player info to existing player
            self.send_to(*other_eid, |tx| send_player_info_add(tx, &snapshot));
            self.send_to(*other_eid, |tx| send_spawn_entity(tx, &snapshot));
            self.send_to(*other_eid, |tx| {
                let msg = basalt_types::TextComponent::text(format!("{username} joined the game"))
                    .color(basalt_types::TextColor::Named(
                        basalt_types::NamedColor::Yellow,
                    ));
                let _ = tx.try_send(ServerOutput::SystemChat {
                    content: msg.to_nbt(),
                    action_bar: false,
                });
            });
        }

        // Welcome message
        self.send_to(eid, |tx| {
            let msg = basalt_types::TextComponent::text(format!("Welcome, {username}!")).color(
                basalt_types::TextColor::Named(basalt_types::NamedColor::Gold),
            );
            let _ = tx.try_send(ServerOutput::SystemChat {
                content: msg.to_nbt(),
                action_bar: false,
            });
        });

        // Dispatch PlayerJoinedEvent
        let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);
        let mut event = PlayerJoinedEvent { info: snapshot };
        self.bus.dispatch(&mut event, &ctx);
        self.process_responses(uuid, &ctx.drain_responses());
        self.rebuild_active_chunks();
    }

    /// Sends the initial world data to a newly connected player.
    fn send_initial_world(
        &mut self,
        eid: basalt_ecs::EntityId,
        entity_id: i32,
        position: (f64, f64, f64),
    ) {
        // Login (Play) packet
        let login = ClientboundPlayLogin {
            entity_id,
            is_hardcore: false,
            world_names: vec!["minecraft:overworld".into()],
            max_players: 20,
            view_distance: 10,
            simulation_distance: 10,
            reduced_debug_info: false,
            enable_respawn_screen: true,
            do_limited_crafting: false,
            world_state: ClientboundPlayLoginSpawninfo {
                dimension: 0,
                name: "minecraft:overworld".into(),
                hashed_seed: 0,
                gamemode: 1,
                previous_gamemode: 255,
                is_debug: false,
                is_flat: true,
                death: None,
                portal_cooldown: 0,
                sea_level: 63,
            },
            enforces_secure_chat: false,
        };
        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::Packet(EncodablePacket::new(
                ClientboundPlayLogin::PACKET_ID,
                login,
            )));
        });

        // DeclareCommands
        if !self.declare_commands.is_empty() {
            let dc = self.declare_commands.clone();
            self.send_to(eid, |tx| {
                let _ = tx.try_send(ServerOutput::Raw {
                    id: ClientboundPlayDeclareCommands::PACKET_ID,
                    data: dc,
                });
            });
        }

        // SpawnPosition
        let spawn_y = self.world.spawn_y() as i32;
        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::Packet(EncodablePacket::new(
                basalt_protocol::packets::play::world::ClientboundPlaySpawnPosition::PACKET_ID,
                basalt_protocol::packets::play::world::ClientboundPlaySpawnPosition {
                    location: Position::new(0, spawn_y, 0),
                    angle: 0.0,
                },
            )));
        });

        // GameEvent (wait for chunks)
        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::GameStateChange {
                reason: 13,
                value: 0.0,
            });
        });

        // UpdateViewPosition + chunks
        let cx = (position.0 as i32) >> 4;
        let cz = (position.2 as i32) >> 4;
        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::UpdateViewPosition { cx, cz });
            let _ = tx.try_send(ServerOutput::ChunkBatchStart);
        });

        let mut count = 0i32;
        for dx in -VIEW_RADIUS..=VIEW_RADIUS {
            for dz in -VIEW_RADIUS..=VIEW_RADIUS {
                self.send_chunk_with_entities(eid, cx + dx, cz + dz);
                count += 1;
            }
        }

        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::ChunkBatchFinished { batch_size: count });
        });

        // Track loaded chunks
        if let Some(view) = self.ecs.get_mut::<ChunkView>(eid) {
            for dx in -VIEW_RADIUS..=VIEW_RADIUS {
                for dz in -VIEW_RADIUS..=VIEW_RADIUS {
                    view.loaded_chunks.insert((cx + dx, cz + dz));
                }
            }
        }

        // Position
        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::SetPosition {
                teleport_id: 1,
                x: position.0,
                y: position.1,
                z: position.2,
                yaw: 0.0,
                pitch: 0.0,
            });
        });

        // Sync full inventory
        if let Some(inv) = self.ecs.get::<basalt_ecs::Inventory>(eid) {
            let protocol_slots = inv.to_protocol_slots();
            self.send_to(eid, |tx| {
                let _ = tx.try_send(ServerOutput::SyncInventory {
                    slots: protocol_slots,
                });
            });
        }
    }

    /// Handles a player disconnection.
    fn handle_player_disconnected(&mut self, uuid: Uuid) {
        let Some(eid) = self.ecs.find_by_uuid(uuid) else {
            return;
        };

        let (entity_id, username) = {
            let Some(pr) = self.ecs.get::<basalt_ecs::PlayerRef>(eid) else {
                return;
            };
            (eid as i32, pr.username.clone())
        };

        // Dispatch PlayerLeftEvent
        let ctx = self.make_context(uuid, entity_id, &username, 0.0, 0.0);
        let mut event = PlayerLeftEvent {
            uuid,
            entity_id,
            username: username.clone(),
        };
        self.bus.dispatch(&mut event, &ctx);
        self.process_responses(uuid, &ctx.drain_responses());

        self.ecs.despawn(eid);
        self.rebuild_active_chunks();

        // Broadcast leave to remaining players
        let remove_players = Arc::new(SharedBroadcast::new(BroadcastEvent::RemovePlayers {
            uuids: vec![uuid],
        }));
        let remove_entities = Arc::new(SharedBroadcast::new(BroadcastEvent::RemoveEntities {
            entity_ids: vec![entity_id],
        }));
        let msg = basalt_types::TextComponent::text(format!("{username} left the game")).color(
            basalt_types::TextColor::Named(basalt_types::NamedColor::Yellow),
        );
        let leave_chat = Arc::new(SharedBroadcast::new(BroadcastEvent::SystemChat {
            content: msg.to_nbt(),
            action_bar: false,
        }));

        for (other_eid, _) in self.ecs.iter::<OutputHandle>() {
            self.send_to(other_eid, |tx| {
                let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&remove_players)));
                let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&remove_entities)));
                let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&leave_chat)));
            });
        }
    }

    // ── Movement ──────────────────────────────────────────────────────

    /// Handles movement input: updates ECS, broadcasts, checks chunk boundaries.
    fn handle_movement(
        &mut self,
        uuid: Uuid,
        pos: Option<(f64, f64, f64)>,
        look: Option<(f32, f32)>,
        on_ground: bool,
    ) {
        let Some(eid) = self.ecs.find_by_uuid(uuid) else {
            return;
        };

        let (entity_id, old_cx, old_cz, x, y, z, yaw, pitch, username) = {
            let Some(p) = self.ecs.get::<basalt_ecs::Position>(eid) else {
                return;
            };
            let old_cx = (p.x as i32) >> 4;
            let old_cz = (p.z as i32) >> 4;
            let Some(r) = self.ecs.get::<basalt_ecs::Rotation>(eid) else {
                return;
            };
            let Some(pr) = self.ecs.get::<basalt_ecs::PlayerRef>(eid) else {
                return;
            };
            (
                eid as i32,
                old_cx,
                old_cz,
                pos.map_or(p.x, |p| p.0),
                pos.map_or(p.y, |p| p.1),
                pos.map_or(p.z, |p| p.2),
                look.map_or(r.yaw, |l| l.0),
                look.map_or(r.pitch, |l| l.1),
                pr.username.clone(),
            )
        };

        // Update ECS
        if let Some(p) = self.ecs.get_mut::<basalt_ecs::Position>(eid) {
            p.x = x;
            p.y = y;
            p.z = z;
        }
        if let Some(r) = self.ecs.get_mut::<basalt_ecs::Rotation>(eid) {
            r.yaw = yaw;
            r.pitch = pitch;
        }

        // Dispatch PlayerMovedEvent
        let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);
        let mut event = PlayerMovedEvent {
            entity_id,
            x,
            y,
            z,
            yaw,
            pitch,
            on_ground,
            old_cx,
            old_cz,
        };
        self.bus.dispatch(&mut event, &ctx);
        let responses = ctx.drain_responses();
        self.process_responses(uuid, &responses);

        // Broadcast movement to other players
        let moved = Arc::new(SharedBroadcast::new(BroadcastEvent::EntityMoved {
            entity_id,
            x,
            y,
            z,
            yaw,
            pitch,
            on_ground,
        }));
        for (other_eid, _) in self.ecs.iter::<OutputHandle>() {
            if other_eid != eid {
                self.send_to(other_eid, |tx| {
                    let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&moved)));
                });
            }
        }

        // Check chunk boundary for streaming
        let new_cx = (x as i32) >> 4;
        let new_cz = (z as i32) >> 4;
        if new_cx != old_cx || new_cz != old_cz {
            self.stream_chunks(eid, new_cx, new_cz);
            self.rebuild_active_chunks();
        }
    }

    /// Streams chunks when a player crosses a chunk boundary.
    fn stream_chunks(&mut self, eid: basalt_ecs::EntityId, new_cx: i32, new_cz: i32) {
        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::UpdateViewPosition {
                cx: new_cx,
                cz: new_cz,
            });
        });

        let r = VIEW_RADIUS;
        let mut in_view = HashSet::new();
        for dx in -r..=r {
            for dz in -r..=r {
                in_view.insert((new_cx + dx, new_cz + dz));
            }
        }

        // Unload
        let Some(view) = self.ecs.get::<ChunkView>(eid) else {
            return;
        };
        let to_unload: Vec<(i32, i32)> = view
            .loaded_chunks
            .iter()
            .filter(|k| !in_view.contains(k))
            .copied()
            .collect();

        for &(cx, cz) in &to_unload {
            self.send_to(eid, |tx| {
                let _ = tx.try_send(ServerOutput::UnloadChunk { cx, cz });
            });
        }

        let Some(view) = self.ecs.get_mut::<ChunkView>(eid) else {
            return;
        };
        for k in &to_unload {
            view.loaded_chunks.remove(k);
        }

        // Load
        let mut to_load = Vec::new();
        for &key in &in_view {
            if view.loaded_chunks.insert(key) {
                to_load.push(key);
            }
        }

        if !to_load.is_empty() {
            self.send_to(eid, |tx| {
                let _ = tx.try_send(ServerOutput::ChunkBatchStart);
            });
            for &(cx, cz) in &to_load {
                self.send_chunk_with_entities(eid, cx, cz);
            }
            self.send_to(eid, |tx| {
                let _ = tx.try_send(ServerOutput::ChunkBatchFinished {
                    batch_size: to_load.len() as i32,
                });
            });
        }
    }

    // ── Blocks ────────────────────────────────────────────────────────

    /// Handles a block dig (break).
    fn handle_block_dig(&mut self, uuid: Uuid, x: i32, y: i32, z: i32, sequence: i32) {
        let Some(eid) = self.ecs.find_by_uuid(uuid) else {
            return;
        };
        let (entity_id, username) = {
            let Some(pr) = self.ecs.get::<basalt_ecs::PlayerRef>(eid) else {
                return;
            };
            (eid as i32, pr.username.clone())
        };

        let original_state = self.world.get_block(x, y, z);
        let ctx = ServerContext::new(Arc::clone(&self.world), uuid, entity_id, username, 0.0, 0.0);
        let mut event = BlockBrokenEvent {
            x,
            y,
            z,
            block_state: original_state,
            sequence,
            player_uuid: uuid,
            cancelled: false,
        };
        self.bus.dispatch(&mut event, &ctx);

        if event.is_cancelled() {
            if let Some(handle) = self.ecs.get::<OutputHandle>(eid) {
                let _ = handle.tx.try_send(ServerOutput::BlockChanged {
                    x,
                    y,
                    z,
                    state: i32::from(original_state),
                });
            }
            return;
        }

        self.process_responses(uuid, &ctx.drain_responses());

        // Collect items to drop from block entity before removing it
        let items_to_drop: Vec<(i32, i32)> = self
            .world
            .get_block_entity(x, y, z)
            .map(|be| match &*be {
                basalt_world::block_entity::BlockEntity::Chest { slots } => slots
                    .iter()
                    .filter_map(|s| s.item_id.map(|id| (id, s.item_count)))
                    .collect(),
            })
            .unwrap_or_default();

        self.world.remove_block_entity(x, y, z);

        // Spawn dropped items for chest contents
        for (item_id, count) in items_to_drop {
            self.spawn_item_entity(x, y, z, item_id, count);
        }

        // If this was part of a double chest, revert the other half to single
        if basalt_world::block::is_chest(original_state)
            && basalt_world::block::chest_type(original_state) != 0
        {
            let facing = basalt_world::block::chest_facing(original_state);
            let offsets = basalt_world::block::chest_adjacent_offsets(facing);
            for (dx, dz) in offsets {
                let nx = x + dx;
                let nz = z + dz;
                let neighbor = self.world.get_block(nx, y, nz);
                if basalt_world::block::is_chest(neighbor)
                    && basalt_world::block::chest_facing(neighbor) == facing
                    && basalt_world::block::chest_type(neighbor) != 0
                {
                    let single = basalt_world::block::chest_state(facing, 0);
                    self.world.set_block(nx, y, nz, single);
                    self.chunk_cache.invalidate(nx >> 4, nz >> 4);
                    let bc = Arc::new(SharedBroadcast::new(BroadcastEvent::BlockChanged {
                        x: nx,
                        y,
                        z: nz,
                        state: i32::from(single),
                    }));
                    for (e, _) in self.ecs.iter::<OutputHandle>() {
                        self.send_to(e, |tx| {
                            let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&bc)));
                        });
                    }
                    break;
                }
            }
        }
    }

    /// Handles a block place.
    fn handle_block_place(
        &mut self,
        uuid: Uuid,
        x: i32,
        y: i32,
        z: i32,
        direction: i32,
        sequence: i32,
    ) {
        let Some(eid) = self.ecs.find_by_uuid(uuid) else {
            return;
        };

        // Check if the clicked block is an interactable container
        // Sneaking players skip interaction and place blocks instead
        let is_sneaking = self.ecs.has::<Sneaking>(eid);
        let clicked_state = self.world.get_block(x, y, z);
        if !is_sneaking && basalt_world::block::is_chest(clicked_state) {
            self.open_chest(eid, x, y, z);
            return;
        }

        let (dx, dy, dz) = face_offset(direction);
        let (px, py, pz) = (x + dx, y + dy, z + dz);

        let (entity_id, username, block_state) = {
            let Some(inv) = self.ecs.get::<basalt_ecs::Inventory>(eid) else {
                return;
            };
            let Some(item_id) = inv.held_item().item_id else {
                return;
            };
            let Some(mut block_state) = basalt_world::block::item_to_default_block_state(item_id)
            else {
                return;
            };
            // Orient directional blocks based on player yaw
            if basalt_world::block::is_chest(block_state) {
                let yaw = self
                    .ecs
                    .get::<basalt_ecs::Rotation>(eid)
                    .map_or(0.0, |r| r.yaw);
                block_state = basalt_world::block::chest_state_for_yaw(yaw);
            }
            let Some(pr) = self.ecs.get::<basalt_ecs::PlayerRef>(eid) else {
                return;
            };
            (eid as i32, pr.username.clone(), block_state)
        };

        let ctx = ServerContext::new(Arc::clone(&self.world), uuid, entity_id, username, 0.0, 0.0);
        let mut event = BlockPlacedEvent {
            x: px,
            y: py,
            z: pz,
            block_state,
            sequence,
            player_uuid: uuid,
            cancelled: false,
        };
        self.bus.dispatch(&mut event, &ctx);

        if event.is_cancelled() {
            if let Some(handle) = self.ecs.get::<OutputHandle>(eid) {
                let _ = handle.tx.try_send(ServerOutput::BlockChanged {
                    x: px,
                    y: py,
                    z: pz,
                    state: i32::from(basalt_world::block::AIR),
                });
            }
            return;
        }

        self.process_responses(uuid, &ctx.drain_responses());

        // Create block entity for interactive blocks (chests)
        if basalt_world::block::is_chest(block_state) {
            self.world.set_block_entity(
                px,
                py,
                pz,
                basalt_world::block_entity::BlockEntity::empty_chest(),
            );

            // Double chest pairing logic:
            // - Not sneaking: scan adjacent blocks for a single chest to pair with
            // - Sneaking + clicked a chest: pair only with the clicked chest
            // - Sneaking + clicked non-chest: no pairing (single chest)
            let facing = basalt_world::block::chest_facing(block_state);
            let mut paired = false;

            // Build candidate list: either all adjacent or just the clicked chest
            let candidates: Vec<(i32, i32)> = if !is_sneaking {
                basalt_world::block::chest_adjacent_offsets(facing)
                    .iter()
                    .map(|&(ddx, ddz)| (px + ddx, pz + ddz))
                    .collect()
            } else if basalt_world::block::is_chest(clicked_state) {
                // Sneaking on a chest: pair only if new chest is lateral (left/right)
                let valid_offsets = basalt_world::block::chest_adjacent_offsets(facing);
                let actual_offset = (px - x, pz - z);
                if valid_offsets.contains(&actual_offset) {
                    vec![(x, z)]
                } else {
                    vec![] // front/back placement: no pairing
                }
            } else {
                vec![] // sneaking on non-chest: no pairing
            };

            for &(nx, nz) in &candidates {
                let neighbor = self.world.get_block(nx, py, nz);
                if basalt_world::block::is_single_chest(neighbor)
                    && basalt_world::block::chest_facing(neighbor) == facing
                {
                    // Compute offset from new chest to neighbor
                    let ddx = nx - px;
                    let ddz = nz - pz;
                    let (new_type, existing_type) =
                        basalt_world::block::chest_double_types(facing, ddx, ddz);
                    let new_state = basalt_world::block::chest_state(facing, new_type);
                    self.world.set_block(px, py, pz, new_state);
                    let neighbor_state = basalt_world::block::chest_state(facing, existing_type);
                    self.world.set_block(nx, py, nz, neighbor_state);
                    self.chunk_cache.invalidate(px >> 4, pz >> 4);
                    self.chunk_cache.invalidate(nx >> 4, nz >> 4);
                    for (e, _) in self.ecs.iter::<OutputHandle>() {
                        self.send_to(e, |tx| {
                            let _ = tx.try_send(ServerOutput::BlockChanged {
                                x: px,
                                y: py,
                                z: pz,
                                state: i32::from(new_state),
                            });
                            let _ = tx.try_send(ServerOutput::BlockEntityData {
                                x: px,
                                y: py,
                                z: pz,
                                action: 2,
                            });
                            let _ = tx.try_send(ServerOutput::BlockChanged {
                                x: nx,
                                y: py,
                                z: nz,
                                state: i32::from(neighbor_state),
                            });
                            let _ = tx.try_send(ServerOutput::BlockEntityData {
                                x: nx,
                                y: py,
                                z: nz,
                                action: 2,
                            });
                        });
                    }
                    paired = true;
                    break;
                }
            }

            if !paired {
                // Single chest — broadcast normally
                for (e, _) in self.ecs.iter::<OutputHandle>() {
                    self.send_to(e, |tx| {
                        let _ = tx.try_send(ServerOutput::BlockChanged {
                            x: px,
                            y: py,
                            z: pz,
                            state: i32::from(block_state),
                        });
                        let _ = tx.try_send(ServerOutput::BlockEntityData {
                            x: px,
                            y: py,
                            z: pz,
                            action: 2,
                        });
                    });
                }
            }
        }
    }

    // ── Response processing ───────────────────────────────────────────

    /// Processes event handler responses.
    fn process_responses(&mut self, source_uuid: Uuid, responses: &[Response]) {
        for response in responses {
            match response {
                Response::Broadcast(basalt_api::BroadcastMessage::BlockChanged {
                    x,
                    y,
                    z,
                    block_state,
                }) => {
                    // Invalidate chunk cache for this block's chunk
                    self.chunk_cache.invalidate(*x >> 4, *z >> 4);
                    let bc = Arc::new(SharedBroadcast::new(BroadcastEvent::BlockChanged {
                        x: *x,
                        y: *y,
                        z: *z,
                        state: *block_state,
                    }));
                    for (e, _) in self.ecs.iter::<OutputHandle>() {
                        self.send_to(e, |tx| {
                            let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&bc)));
                        });
                    }
                }
                Response::Broadcast(basalt_api::BroadcastMessage::Chat { content }) => {
                    let bc = Arc::new(SharedBroadcast::new(BroadcastEvent::SystemChat {
                        content: content.clone(),
                        action_bar: false,
                    }));
                    for (e, _) in self.ecs.iter::<OutputHandle>() {
                        self.send_to(e, |tx| {
                            let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&bc)));
                        });
                    }
                }
                Response::Broadcast(_) => {}
                Response::SendBlockAck { sequence } => {
                    if let Some(eid) = self.ecs.find_by_uuid(source_uuid)
                        && let Some(handle) = self.ecs.get::<OutputHandle>(eid)
                    {
                        let _ = handle.tx.try_send(ServerOutput::BlockAck {
                            sequence: *sequence,
                        });
                    }
                }
                Response::SendSystemChat {
                    content,
                    action_bar,
                } => {
                    if let Some(eid) = self.ecs.find_by_uuid(source_uuid)
                        && let Some(handle) = self.ecs.get::<OutputHandle>(eid)
                    {
                        let _ = handle.tx.try_send(ServerOutput::SystemChat {
                            content: content.clone(),
                            action_bar: *action_bar,
                        });
                    }
                }
                Response::SendPosition {
                    teleport_id,
                    x,
                    y,
                    z,
                    yaw,
                    pitch,
                } => {
                    if let Some(eid) = self.ecs.find_by_uuid(source_uuid) {
                        if let Some(pos) = self.ecs.get_mut::<basalt_ecs::Position>(eid) {
                            pos.x = *x;
                            pos.y = *y;
                            pos.z = *z;
                        }
                        if let Some(handle) = self.ecs.get::<OutputHandle>(eid) {
                            let _ = handle.tx.try_send(ServerOutput::SetPosition {
                                teleport_id: *teleport_id,
                                x: *x,
                                y: *y,
                                z: *z,
                                yaw: *yaw,
                                pitch: *pitch,
                            });
                        }
                    }
                }
                Response::StreamChunks { new_cx, new_cz } => {
                    if let Some(eid) = self.ecs.find_by_uuid(source_uuid) {
                        self.stream_chunks(eid, *new_cx, *new_cz);
                    }
                }
                Response::SendGameStateChange { reason, value } => {
                    if let Some(eid) = self.ecs.find_by_uuid(source_uuid)
                        && let Some(handle) = self.ecs.get::<OutputHandle>(eid)
                    {
                        let _ = handle.tx.try_send(ServerOutput::GameStateChange {
                            reason: *reason,
                            value: *value,
                        });
                    }
                }
                Response::PersistChunk { cx, cz } => {
                    let _ = self
                        .io_tx
                        .send(crate::runtime::io_thread::IoRequest::PersistChunk {
                            cx: *cx,
                            cz: *cz,
                        });
                }
                Response::SpawnDroppedItem {
                    x,
                    y,
                    z,
                    item_id,
                    count,
                } => {
                    self.spawn_item_entity(*x, *y, *z, *item_id, *count);
                }
                Response::OpenChest { x, y, z } => {
                    if let Some(eid) = self.ecs.find_by_uuid(source_uuid) {
                        self.open_chest(eid, *x, *y, *z);
                    }
                }
            }
        }
    }

    /// Spawns a dropped item entity and broadcasts it to all players.
    fn spawn_item_entity(&mut self, x: i32, y: i32, z: i32, item_id: i32, count: i32) {
        use std::sync::atomic::Ordering;

        let entity_id = self.next_entity_id.fetch_add(1, Ordering::Relaxed);
        let eid = entity_id as basalt_ecs::EntityId;

        // Small random offset so items don't stack perfectly
        let px = x as f64 + 0.5;
        let py = y as f64 + 0.25;
        let pz = z as f64 + 0.5;

        self.ecs.spawn_with_id(eid);
        self.ecs.set(
            eid,
            basalt_ecs::Position {
                x: px,
                y: py,
                z: pz,
            },
        );
        self.ecs.set(
            eid,
            basalt_ecs::Velocity {
                dx: 0.0,
                dy: 0.2,
                dz: 0.0,
            },
        );
        self.ecs.set(
            eid,
            basalt_ecs::BoundingBox {
                width: 0.25,
                height: 0.25,
            },
        );
        self.ecs.set(eid, basalt_ecs::EntityKind { type_id: 68 });
        self.ecs.set(
            eid,
            basalt_ecs::PickupDelay {
                remaining_ticks: 10,
            },
        );
        self.ecs.set(
            eid,
            basalt_ecs::Lifetime {
                remaining_ticks: 6000,
            },
        );
        self.ecs.set(
            eid,
            basalt_ecs::DroppedItem {
                slot: basalt_types::Slot::new(item_id, count),
            },
        );

        // Broadcast spawn to all players
        let bc = Arc::new(SharedBroadcast::new(BroadcastEvent::SpawnItemEntity {
            entity_id,
            x: px,
            y: py,
            z: pz,
            vx: 0.0,
            vy: 0.2,
            vz: 0.0,
            item_id,
            count,
        }));
        for (other_eid, _) in self.ecs.iter::<OutputHandle>() {
            self.send_to(other_eid, |tx| {
                let _ = tx.try_send(ServerOutput::Broadcast(Arc::clone(&bc)));
            });
        }
    }

    // ── Containers ─────────────────────────────────────────────────────

    /// Opens a chest container for a player.
    ///
    /// Creates a block entity if it doesn't exist yet, assigns a window
    /// ID, and sends OpenWindow + SetContainerContent to the client.
    fn open_chest(&mut self, eid: basalt_ecs::EntityId, x: i32, y: i32, z: i32) {
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
    fn open_container(&mut self, eid: basalt_ecs::EntityId, view: &ContainerView) {
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
    fn build_chest_view(&self, x: i32, y: i32, z: i32) -> ContainerView {
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
    fn handle_container_click(
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
    fn notify_container_viewers(
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

    // ── Helpers ────────────────────────────────────────────────────────

    /// Sends output to a player entity via their OutputHandle.
    /// Sends a chunk to a player and follows up with BlockEntityData
    /// for any block entities in that chunk (chests, etc.).
    fn send_chunk_with_entities(&self, eid: basalt_ecs::EntityId, cx: i32, cz: i32) {
        // Force chunk + block entities to be loaded from disk before querying
        self.world.with_chunk(cx, cz, |_| {});

        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::SendChunk { cx, cz });
        });
        // Send block entity data for chests in this chunk
        for (x, y, z, be) in self.world.block_entities_in_chunk(cx, cz) {
            let action = match &be {
                basalt_world::block_entity::BlockEntity::Chest { .. } => 2,
            };
            self.send_to(eid, |tx| {
                let _ = tx.try_send(ServerOutput::BlockEntityData { x, y, z, action });
            });
        }
    }

    fn send_to(&self, eid: basalt_ecs::EntityId, f: impl FnOnce(&mpsc::Sender<ServerOutput>)) {
        if let Some(handle) = self.ecs.get::<OutputHandle>(eid) {
            f(&handle.tx);
        }
    }

    /// Creates a ServerContext for event dispatch.
    fn make_context(
        &self,
        uuid: Uuid,
        entity_id: i32,
        username: &str,
        yaw: f32,
        pitch: f32,
    ) -> ServerContext {
        ServerContext::new(
            Arc::clone(&self.world),
            uuid,
            entity_id,
            username.to_string(),
            yaw,
            pitch,
        )
    }
}

/// Returns the (dx, dy, dz) offset for a block face direction.
fn face_offset(direction: i32) -> (i32, i32, i32) {
    match direction {
        0 => (0, -1, 0),
        1 => (0, 1, 0),
        2 => (0, 0, -1),
        3 => (0, 0, 1),
        4 => (-1, 0, 0),
        5 => (1, 0, 0),
        _ => (0, 0, 0),
    }
}

/// Sends a PlayerInfo "add player" packet (manually encoded due to switch fields).
fn send_player_info_add(output_tx: &mpsc::Sender<ServerOutput>, info: &basalt_api::PlayerSnapshot) {
    use basalt_protocol::packets::play::player::ClientboundPlayPlayerInfo;

    let mut buf = Vec::new();
    let actions: u8 = 0x01 | 0x04 | 0x08;
    actions.encode(&mut buf).unwrap();
    VarInt(1).encode(&mut buf).unwrap();
    info.uuid.encode(&mut buf).unwrap();
    info.username.encode(&mut buf).unwrap();
    VarInt(info.skin_properties.len() as i32)
        .encode(&mut buf)
        .unwrap();
    for prop in &info.skin_properties {
        prop.name.encode(&mut buf).unwrap();
        prop.value.encode(&mut buf).unwrap();
        if let Some(sig) = &prop.signature {
            true.encode(&mut buf).unwrap();
            sig.encode(&mut buf).unwrap();
        } else {
            false.encode(&mut buf).unwrap();
        }
    }
    VarInt(1).encode(&mut buf).unwrap(); // gamemode: creative
    true.encode(&mut buf).unwrap(); // listed
    let _ = output_tx.try_send(ServerOutput::Raw {
        id: ClientboundPlayPlayerInfo::PACKET_ID,
        data: buf,
    });
}

/// Sends a SpawnEntity packet for a player entity.
fn send_spawn_entity(output_tx: &mpsc::Sender<ServerOutput>, info: &basalt_api::PlayerSnapshot) {
    let packet = ClientboundPlaySpawnEntity {
        entity_id: info.entity_id,
        object_uuid: info.uuid,
        r#type: 147,
        x: info.x,
        y: info.y,
        z: info.z,
        pitch: angle_to_byte(info.pitch),
        yaw: (info.yaw / 360.0 * 256.0) as i8,
        head_pitch: 0,
        object_data: 0,
        velocity: Vec3i16 { x: 0, y: 0, z: 0 },
    };
    let _ = output_tx.try_send(ServerOutput::Packet(EncodablePacket::new(
        ClientboundPlaySpawnEntity::PACKET_ID,
        packet,
    )));
}

#[cfg(test)]
mod tests {
    use super::*;
    use basalt_api::Plugin;

    fn test_game_loop() -> (
        GameLoop,
        mpsc::UnboundedSender<GameInput>,
        mpsc::UnboundedReceiver<crate::runtime::io_thread::IoRequest>,
    ) {
        let world = Arc::new(basalt_world::World::new_memory(42));
        let chunk_cache = Arc::new(ChunkPacketCache::new(Arc::clone(&world), 256));
        let (game_tx, game_rx) = mpsc::unbounded_channel();
        let (io_tx, io_rx) = mpsc::unbounded_channel();

        let mut instant_bus = EventBus::new();
        let mut bus = EventBus::new();
        let mut commands = Vec::new();
        let mut systems = Vec::new();
        let mut components = Vec::new();
        {
            let mut registrar = basalt_api::PluginRegistrar::new(
                &mut instant_bus,
                &mut bus,
                &mut commands,
                &mut systems,
                &mut components,
                Arc::clone(&world),
            );
            basalt_plugin_block::BlockPlugin.on_enable(&mut registrar);
            basalt_plugin_drops::DropsPlugin.on_enable(&mut registrar);
        }

        let mut ecs = basalt_ecs::Ecs::new();
        // Register core systems (same as lib.rs)
        ecs.add_system(
            basalt_ecs::SystemBuilder::new("lifetime")
                .phase(basalt_ecs::Phase::Simulate)
                .every(1)
                .run(|ecs: &mut basalt_ecs::Ecs| {
                    for (_, lt) in ecs.iter_mut::<basalt_ecs::Lifetime>() {
                        if lt.remaining_ticks > 0 {
                            lt.remaining_ticks -= 1;
                        }
                    }
                }),
        );
        ecs.add_system(
            basalt_ecs::SystemBuilder::new("pickup_delay")
                .phase(basalt_ecs::Phase::Simulate)
                .every(1)
                .run(|ecs: &mut basalt_ecs::Ecs| {
                    for (_, delay) in ecs.iter_mut::<basalt_ecs::PickupDelay>() {
                        if delay.remaining_ticks > 0 {
                            delay.remaining_ticks -= 1;
                        }
                    }
                }),
        );
        let game_loop = GameLoop::new(
            bus,
            world,
            chunk_cache,
            game_rx,
            io_tx,
            ecs,
            Vec::new(),
            Arc::new(std::sync::atomic::AtomicI32::new(1000)),
            8,
            0,
        );
        (game_loop, game_tx, io_rx)
    }

    fn connect_player(
        game_loop: &mut GameLoop,
        game_tx: &mpsc::UnboundedSender<GameInput>,
        uuid: Uuid,
        entity_id: i32,
    ) -> mpsc::Receiver<ServerOutput> {
        let (output_tx, output_rx) = mpsc::channel(256);
        let _ = game_tx.send(GameInput::PlayerConnected {
            entity_id,
            uuid,
            username: "Steve".into(),
            skin_properties: vec![],
            position: (0.0, -60.0, 0.0),
            yaw: 0.0,
            pitch: 0.0,
            output_tx,
        });
        game_loop.tick(0);
        output_rx
    }

    #[test]
    fn player_connect_and_disconnect() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);
        assert!(game_loop.ecs.find_by_uuid(uuid).is_some());

        let _ = game_tx.send(GameInput::PlayerDisconnected { uuid });
        game_loop.tick(1);
        assert!(game_loop.ecs.find_by_uuid(uuid).is_none());
    }

    #[test]
    fn block_dig_sets_air() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::STONE);

        let _ = game_tx.send(GameInput::BlockDig {
            uuid,
            status: 0,
            x: 5,
            y: 64,
            z: 3,
            sequence: 42,
        });
        game_loop.tick(2);
        assert_eq!(
            game_loop.world.get_block(5, 64, 3),
            basalt_world::block::AIR
        );
    }

    #[test]
    fn held_item_slot_change() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let _ = game_tx.send(GameInput::HeldItemSlot { uuid, slot: 3 });
        game_loop.tick(1);

        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        let inv = game_loop.ecs.get::<basalt_ecs::Inventory>(eid).unwrap();
        assert_eq!(inv.held_slot, 3);
    }

    #[test]
    fn face_offset_all_directions() {
        assert_eq!(face_offset(0), (0, -1, 0));
        assert_eq!(face_offset(1), (0, 1, 0));
        assert_eq!(face_offset(2), (0, 0, -1));
        assert_eq!(face_offset(3), (0, 0, 1));
        assert_eq!(face_offset(4), (-1, 0, 0));
        assert_eq!(face_offset(5), (1, 0, 0));
        assert_eq!(face_offset(99), (0, 0, 0));
    }

    #[test]
    fn player_connect_sends_initial_world() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let mut count = 0;
        while rx.try_recv().is_ok() {
            count += 1;
        }
        assert!(count > 10, "expected many initial packets, got {count}");
    }

    #[test]
    fn player_connect_creates_all_components() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        assert!(game_loop.ecs.has::<basalt_ecs::Position>(eid));
        assert!(game_loop.ecs.has::<basalt_ecs::Rotation>(eid));
        assert!(game_loop.ecs.has::<basalt_ecs::BoundingBox>(eid));
        assert!(game_loop.ecs.has::<basalt_ecs::Inventory>(eid));
        assert!(game_loop.ecs.has::<basalt_ecs::PlayerRef>(eid));
        assert!(game_loop.ecs.has::<SkinData>(eid));
        assert!(game_loop.ecs.has::<ChunkView>(eid));
        assert!(game_loop.ecs.has::<OutputHandle>(eid));
    }

    #[test]
    fn movement_updates_position_and_rotation() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let _ = game_tx.send(GameInput::PositionLook {
            uuid,
            x: 10.0,
            y: 65.0,
            z: -5.0,
            yaw: 90.0,
            pitch: 45.0,
            on_ground: true,
        });
        game_loop.tick(1);

        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        let pos = game_loop.ecs.get::<basalt_ecs::Position>(eid).unwrap();
        assert_eq!(pos.x, 10.0);
        assert_eq!(pos.y, 65.0);
        assert_eq!(pos.z, -5.0);
        let rot = game_loop.ecs.get::<basalt_ecs::Rotation>(eid).unwrap();
        assert_eq!(rot.yaw, 90.0);
        assert_eq!(rot.pitch, 45.0);
    }

    #[test]
    fn look_only_updates_rotation() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let _ = game_tx.send(GameInput::Look {
            uuid,
            yaw: 180.0,
            pitch: -30.0,
            on_ground: true,
        });
        game_loop.tick(1);

        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        let rot = game_loop.ecs.get::<basalt_ecs::Rotation>(eid).unwrap();
        assert_eq!(rot.yaw, 180.0);
        assert_eq!(rot.pitch, -30.0);
        let pos = game_loop.ecs.get::<basalt_ecs::Position>(eid).unwrap();
        assert_eq!(pos.x, 0.0);
    }

    #[test]
    fn two_players_join_broadcast() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid1 = Uuid::from_bytes([1; 16]);
        let uuid2 = Uuid::from_bytes([2; 16]);
        let mut rx1 = connect_player(&mut game_loop, &game_tx, uuid1, 1);

        while rx1.try_recv().is_ok() {}

        let _rx2 = connect_player(&mut game_loop, &game_tx, uuid2, 2);

        let mut p1_count = 0;
        while rx1.try_recv().is_ok() {
            p1_count += 1;
        }
        assert!(
            p1_count >= 3,
            "player 1 should receive join broadcast, got {p1_count} packets"
        );
    }

    #[test]
    fn player_disconnect_broadcasts_leave() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid1 = Uuid::from_bytes([1; 16]);
        let uuid2 = Uuid::from_bytes([2; 16]);
        let mut rx1 = connect_player(&mut game_loop, &game_tx, uuid1, 1);
        let _rx2 = connect_player(&mut game_loop, &game_tx, uuid2, 2);

        while rx1.try_recv().is_ok() {}

        let _ = game_tx.send(GameInput::PlayerDisconnected { uuid: uuid2 });
        game_loop.tick(2);

        let mut p1_count = 0;
        while rx1.try_recv().is_ok() {
            p1_count += 1;
        }
        assert!(
            p1_count >= 3,
            "player 1 should receive leave broadcast, got {p1_count}"
        );
    }

    #[test]
    fn movement_broadcasts_to_other_players() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid1 = Uuid::from_bytes([1; 16]);
        let uuid2 = Uuid::from_bytes([2; 16]);
        let _rx1 = connect_player(&mut game_loop, &game_tx, uuid1, 1);
        let mut rx2 = connect_player(&mut game_loop, &game_tx, uuid2, 2);

        while rx2.try_recv().is_ok() {}

        let _ = game_tx.send(GameInput::Position {
            uuid: uuid1,
            x: 5.0,
            y: -60.0,
            z: 3.0,
            on_ground: true,
        });
        game_loop.tick(2);

        let mut got_moved = false;
        while let Ok(msg) = rx2.try_recv() {
            if matches!(msg, ServerOutput::Broadcast(_)) {
                got_moved = true;
            }
        }
        assert!(got_moved, "player 2 should receive movement broadcast");
    }

    #[test]
    fn block_place_with_held_item() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        if let Some(inv) = game_loop.ecs.get_mut::<basalt_ecs::Inventory>(eid) {
            inv.hotbar_mut()[0] = basalt_types::Slot {
                item_id: Some(1),
                item_count: 1,
                component_data: vec![],
            };
        }

        let _ = game_tx.send(GameInput::BlockPlace {
            uuid,
            x: 5,
            y: 63,
            z: 3,
            direction: 1,
            sequence: 10,
        });
        game_loop.tick(2);

        assert_eq!(
            game_loop.world.get_block(5, 64, 3),
            basalt_world::block::STONE
        );
    }

    #[test]
    fn set_creative_slot() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let _ = game_tx.send(GameInput::SetCreativeSlot {
            uuid,
            slot: 36,
            item: basalt_types::Slot {
                item_id: Some(1),
                item_count: 64,
                component_data: vec![],
            },
        });
        game_loop.tick(1);

        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        let inv = game_loop.ecs.get::<basalt_ecs::Inventory>(eid).unwrap();
        assert_eq!(inv.hotbar()[0].item_id, Some(1));
        assert_eq!(inv.hotbar()[0].item_count, 64);
    }

    #[test]
    fn set_creative_slot_out_of_range_ignored() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let _ = game_tx.send(GameInput::SetCreativeSlot {
            uuid,
            slot: 10,
            item: basalt_types::Slot {
                item_id: Some(1),
                item_count: 1,
                component_data: vec![],
            },
        });
        game_loop.tick(1);

        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        let inv = game_loop.ecs.get::<basalt_ecs::Inventory>(eid).unwrap();
        assert!(inv.hotbar()[0].item_id.is_none());
    }

    #[test]
    fn block_dig_sends_ack_and_broadcast() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::STONE);

        while rx.try_recv().is_ok() {}

        let _ = game_tx.send(GameInput::BlockDig {
            uuid,
            status: 0,
            x: 5,
            y: 64,
            z: 3,
            sequence: 42,
        });
        game_loop.tick(2);

        let mut got_ack = false;
        let mut got_block_change = false;
        while let Ok(msg) = rx.try_recv() {
            match &msg {
                ServerOutput::BlockAck { .. } => got_ack = true,
                ServerOutput::Broadcast(bc) => {
                    if matches!(bc.event, BroadcastEvent::BlockChanged { .. }) {
                        got_block_change = true;
                    }
                }
                _ => {}
            }
        }
        assert!(got_ack, "should have received block ack");
        assert!(
            got_block_change,
            "should have received block change broadcast"
        );
    }

    #[test]
    fn position_update_for_unknown_player_ignored() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let unknown = Uuid::from_bytes([99; 16]);

        let _ = game_tx.send(GameInput::Position {
            uuid: unknown,
            x: 10.0,
            y: 65.0,
            z: -5.0,
            on_ground: true,
        });
        game_loop.tick(0);
    }

    #[test]
    fn block_dig_for_unknown_player_ignored() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let unknown = Uuid::from_bytes([99; 16]);

        let _ = game_tx.send(GameInput::BlockDig {
            uuid: unknown,
            status: 0,
            x: 5,
            y: 64,
            z: 3,
            sequence: 1,
        });
        game_loop.tick(0);
    }

    #[test]
    fn chunk_streaming_on_boundary_crossing() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        while rx.try_recv().is_ok() {}

        let _ = game_tx.send(GameInput::Position {
            uuid,
            x: 32.0,
            y: -60.0,
            z: 0.0,
            on_ground: true,
        });
        game_loop.tick(1);

        let mut got_packets = false;
        while rx.try_recv().is_ok() {
            got_packets = true;
        }
        assert!(
            got_packets,
            "should receive chunk streaming packets on boundary crossing"
        );

        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        let view = game_loop.ecs.get::<ChunkView>(eid).unwrap();
        let new_cx = (32.0_f64 as i32) >> 4;
        assert!(
            view.loaded_chunks.contains(&(new_cx, 0)),
            "chunk view should contain the new center chunk"
        );
    }

    #[test]
    fn q_key_drop_single_item() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        // Give 10 stone in hotbar slot 0
        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_ecs::Inventory>(eid)
            .unwrap()
            .slots[0] = basalt_types::Slot::new(1, 10);

        while rx.try_recv().is_ok() {}

        // Q key = BlockDig status 4 (drop single)
        let _ = game_tx.send(GameInput::BlockDig {
            uuid,
            status: 4,
            x: 0,
            y: 0,
            z: 0,
            sequence: 0,
        });
        game_loop.tick(1);

        let inv = game_loop.ecs.get::<basalt_ecs::Inventory>(eid).unwrap();
        assert_eq!(inv.slots[0].item_count, 9, "should have 9 after dropping 1");

        // Should receive SetSlot + SpawnEntity broadcast
        let mut got_set_slot = false;
        let mut got_spawn = false;
        while let Ok(msg) = rx.try_recv() {
            match &msg {
                ServerOutput::SetSlot { slot, item } => {
                    assert_eq!(*slot, 0);
                    assert_eq!(item.item_count, 9);
                    got_set_slot = true;
                }
                ServerOutput::Broadcast(_) => got_spawn = true,
                _ => {}
            }
        }
        assert!(got_set_slot, "should sync hotbar slot");
        assert!(got_spawn, "should spawn dropped item entity");
    }

    #[test]
    fn ctrl_q_drop_full_stack() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_ecs::Inventory>(eid)
            .unwrap()
            .slots[0] = basalt_types::Slot::new(1, 32);

        while rx.try_recv().is_ok() {}

        // Ctrl+Q = BlockDig status 3 (drop stack)
        let _ = game_tx.send(GameInput::BlockDig {
            uuid,
            status: 3,
            x: 0,
            y: 0,
            z: 0,
            sequence: 0,
        });
        game_loop.tick(1);

        let inv = game_loop.ecs.get::<basalt_ecs::Inventory>(eid).unwrap();
        assert!(
            inv.slots[0].is_empty(),
            "slot should be empty after full drop"
        );
    }

    #[test]
    fn creative_drop_slot_minus_one() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        // Creative drop: SetCreativeSlot with slot -1
        let _ = game_tx.send(GameInput::SetCreativeSlot {
            uuid,
            slot: -1,
            item: basalt_types::Slot::new(1, 5),
        });
        game_loop.tick(1);

        // Should spawn a dropped item entity
        let mut got_spawn = false;
        while let Ok(msg) = rx.try_recv() {
            if matches!(&msg, ServerOutput::Broadcast(_)) {
                got_spawn = true;
            }
        }
        assert!(got_spawn, "creative drop should spawn item entity");
    }

    #[test]
    fn window_click_outside_drops_cursor() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        // Set cursor item directly
        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_ecs::Inventory>(eid)
            .unwrap()
            .cursor = basalt_types::Slot::new(1, 16);

        while rx.try_recv().is_ok() {}

        // Click outside window (slot -999)
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: -999,
            button: 0,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        // Cursor should be empty
        let inv = game_loop.ecs.get::<basalt_ecs::Inventory>(eid).unwrap();
        assert!(
            inv.cursor.is_empty(),
            "cursor should be empty after drop outside"
        );

        // Should spawn item entity
        let mut got_spawn = false;
        while let Ok(msg) = rx.try_recv() {
            if matches!(&msg, ServerOutput::Broadcast(_)) {
                got_spawn = true;
            }
        }
        assert!(got_spawn, "should spawn dropped item from cursor");
    }

    #[test]
    fn window_click_applies_changed_slots() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_ecs::Inventory>(eid)
            .unwrap()
            .slots[0] = basalt_types::Slot::new(1, 10);

        // Swap hotbar slot 0 to main slot 9 (window: 36 → 9)
        let _ = game_tx.send(GameInput::WindowClick {
            uuid,
            slot: 36,
            button: 0,
            mode: 0,
            changed_slots: vec![
                (36, basalt_types::Slot::empty()),
                (9, basalt_types::Slot::new(1, 10)),
            ],
            cursor_item: basalt_types::Slot::empty(),
        });
        game_loop.tick(1);

        let inv = game_loop.ecs.get::<basalt_ecs::Inventory>(eid).unwrap();
        assert!(inv.slots[0].is_empty(), "hotbar 0 should be empty");
        assert_eq!(inv.slots[9].item_id, Some(1), "main 0 should have item");
        assert_eq!(inv.slots[9].item_count, 10);
    }

    #[test]
    fn lifetime_system_despawns_expired() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        // Manually spawn an entity with lifetime = 2
        let eid = 999u32;
        game_loop.ecs.spawn_with_id(eid);
        game_loop.ecs.set(
            eid,
            basalt_ecs::Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        );
        game_loop
            .ecs
            .set(eid, basalt_ecs::Lifetime { remaining_ticks: 2 });

        game_loop.tick(1); // system: 2 → 1, collect: not 0 → alive
        assert!(game_loop.ecs.has::<basalt_ecs::Lifetime>(eid));

        game_loop.tick(2); // system: 1 → 0, collect: 0 → despawned
        assert!(
            !game_loop.ecs.has::<basalt_ecs::Lifetime>(eid),
            "entity should be despawned after lifetime reaches 0"
        );
    }

    #[test]
    fn player_connect_syncs_inventory() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let mut got_sync = false;
        while let Ok(msg) = rx.try_recv() {
            if matches!(&msg, ServerOutput::SyncInventory { .. }) {
                got_sync = true;
            }
        }
        assert!(got_sync, "should receive SyncInventory on connect");
    }

    #[test]
    fn chest_placement_creates_block_entity() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        // Give chest in hotbar slot 0 (item 280 = chest)
        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_ecs::Inventory>(eid)
            .unwrap()
            .slots[0] = basalt_types::Slot::new(313, 1); // chest item ID

        // Place chest on top of (5, -60, 3)
        let _ = game_tx.send(GameInput::BlockPlace {
            uuid,
            x: 5,
            y: -60,
            z: 3,
            direction: 1,
            sequence: 10,
        });
        game_loop.tick(1);

        // Block entity should exist
        assert!(
            game_loop.world.get_block_entity(5, -59, 3).is_some(),
            "chest placement should create block entity"
        );
    }

    #[test]
    fn chest_break_removes_block_entity() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        // Place a chest block + entity manually
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CHEST);
        game_loop.world.set_block_entity(
            5,
            64,
            3,
            basalt_world::block_entity::BlockEntity::empty_chest(),
        );

        // Break it
        let _ = game_tx.send(GameInput::BlockDig {
            uuid,
            status: 0,
            x: 5,
            y: 64,
            z: 3,
            sequence: 1,
        });
        game_loop.tick(1);

        assert!(
            game_loop.world.get_block_entity(5, 64, 3).is_none(),
            "breaking chest should remove block entity"
        );
    }

    #[test]
    fn open_chest_sends_open_window() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);
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
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

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
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);
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
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);
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
    fn chest_orientation_based_on_yaw() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        // Set player yaw to 180 (facing north → chest faces south)
        game_loop
            .ecs
            .get_mut::<basalt_ecs::Rotation>(eid)
            .unwrap()
            .yaw = 180.0;
        // Give chest in hotbar
        game_loop
            .ecs
            .get_mut::<basalt_ecs::Inventory>(eid)
            .unwrap()
            .slots[0] = basalt_types::Slot::new(313, 1);

        let _ = game_tx.send(GameInput::BlockPlace {
            uuid,
            x: 5,
            y: -60,
            z: 3,
            direction: 1,
            sequence: 1,
        });
        game_loop.tick(1);

        // Chest at (5, -59, 3) should face south (state 3016)
        let state = game_loop.world.get_block(5, -59, 3);
        assert_eq!(
            state,
            basalt_world::block::chest_state_for_yaw(180.0),
            "chest should face south when player faces north"
        );
    }

    #[test]
    fn close_window_removes_open_container() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

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
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);
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

    #[test]
    fn chest_break_drops_contents_and_self() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        // Place chest with items inside
        game_loop
            .world
            .set_block(5, 64, 3, basalt_world::block::CHEST);
        let mut be = basalt_world::block_entity::BlockEntity::empty_chest();
        let basalt_world::block_entity::BlockEntity::Chest { ref mut slots } = be;
        slots[0] = basalt_types::Slot::new(42, 16);
        game_loop.world.set_block_entity(5, 64, 3, be);

        // Break it
        let _ = game_tx.send(GameInput::BlockDig {
            uuid,
            status: 0,
            x: 5,
            y: 64,
            z: 3,
            sequence: 1,
        });
        game_loop.tick(1);

        // Block entity removed
        assert!(game_loop.world.get_block_entity(5, 64, 3).is_none());

        // Should have spawned dropped items (chest contents + chest block itself)
        let mut spawn_count = 0;
        while let Ok(msg) = rx.try_recv() {
            if matches!(&msg, ServerOutput::Broadcast(bc) if matches!(bc.event, BroadcastEvent::SpawnItemEntity { .. }))
            {
                spawn_count += 1;
            }
        }
        // At least 2 spawns: 1 for the item inside + 1 for the chest block itself
        assert!(
            spawn_count >= 2,
            "should drop chest contents + chest block, got {spawn_count} spawns"
        );
    }

    #[test]
    fn double_chest_forms_on_adjacent_placement() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        let eid = game_loop.ecs.find_by_uuid(uuid).unwrap();
        game_loop
            .ecs
            .get_mut::<basalt_ecs::Rotation>(eid)
            .unwrap()
            .yaw = 0.0; // facing south → chest faces north

        // Place first chest
        game_loop
            .ecs
            .get_mut::<basalt_ecs::Inventory>(eid)
            .unwrap()
            .slots[0] = basalt_types::Slot::new(313, 2);

        let _ = game_tx.send(GameInput::BlockPlace {
            uuid,
            x: 5,
            y: -60,
            z: 3,
            direction: 1,
            sequence: 1,
        });
        game_loop.tick(1);

        let first_state = game_loop.world.get_block(5, -59, 3);
        assert!(
            basalt_world::block::is_single_chest(first_state),
            "first chest should be single"
        );

        // Place second chest adjacent (east, +X)
        let _ = game_tx.send(GameInput::BlockPlace {
            uuid,
            x: 6,
            y: -60,
            z: 3,
            direction: 1,
            sequence: 2,
        });
        game_loop.tick(2);

        let left = game_loop.world.get_block(5, -59, 3);
        let right = game_loop.world.get_block(6, -59, 3);
        assert!(basalt_world::block::is_chest(left), "left should be chest");
        assert!(
            basalt_world::block::is_chest(right),
            "right should be chest"
        );
        assert_ne!(
            basalt_world::block::chest_type(left),
            0,
            "left should not be single"
        );
        assert_ne!(
            basalt_world::block::chest_type(right),
            0,
            "right should not be single"
        );
        assert_ne!(
            basalt_world::block::chest_type(left),
            basalt_world::block::chest_type(right),
            "left and right should have different types"
        );
    }

    #[test]
    fn breaking_double_chest_reverts_other_to_single() {
        let (mut game_loop, game_tx, _io_rx) = test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = connect_player(&mut game_loop, &game_tx, uuid, 1);

        // Manually place a double chest (north-facing, left at x=5, right at x=6)
        let left_state = basalt_world::block::chest_state(0, 1); // north, left
        let right_state = basalt_world::block::chest_state(0, 2); // north, right
        game_loop.world.set_block(5, 64, 3, left_state);
        game_loop.world.set_block(6, 64, 3, right_state);
        game_loop.world.set_block_entity(
            5,
            64,
            3,
            basalt_world::block_entity::BlockEntity::empty_chest(),
        );
        game_loop.world.set_block_entity(
            6,
            64,
            3,
            basalt_world::block_entity::BlockEntity::empty_chest(),
        );

        // Break the left half
        let _ = game_tx.send(GameInput::BlockDig {
            uuid,
            status: 0,
            x: 5,
            y: 64,
            z: 3,
            sequence: 1,
        });
        game_loop.tick(1);

        // Right half should be single now
        let remaining = game_loop.world.get_block(6, 64, 3);
        assert!(
            basalt_world::block::is_single_chest(remaining),
            "remaining half should revert to single chest"
        );
    }
}
