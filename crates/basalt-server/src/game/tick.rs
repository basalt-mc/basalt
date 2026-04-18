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
            declare_commands,
            active_chunks: HashSet::new(),
            simulation_distance,
            persistence_interval_ticks,
        }
    }

    /// Processes one tick.
    pub fn tick(&mut self, tick: u64) {
        self.drain_game_input();
        self.ecs.run_all(tick);
        self.flush_dirty_chunks_if_due(tick);
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
                } => {
                    if status == 0 {
                        self.handle_block_dig(uuid, x, y, z, sequence);
                    }
                }
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
                    if let Some(eid) = self.ecs.find_by_uuid(uuid)
                        && let Some(inv) = self.ecs.get_mut::<basalt_ecs::Inventory>(eid)
                    {
                        let hotbar_idx = slot - 36;
                        if (0..9).contains(&hotbar_idx) {
                            inv.hotbar[hotbar_idx as usize] = item;
                        }
                    }
                }
            }
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
                self.send_to(eid, |tx| {
                    let _ = tx.try_send(ServerOutput::SendChunk {
                        cx: cx + dx,
                        cz: cz + dz,
                    });
                });
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
                self.send_to(eid, |tx| {
                    let _ = tx.try_send(ServerOutput::SendChunk { cx, cz });
                });
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
        let (dx, dy, dz) = face_offset(direction);
        let (px, py, pz) = (x + dx, y + dy, z + dz);

        let (entity_id, username, block_state) = {
            let Some(inv) = self.ecs.get::<basalt_ecs::Inventory>(eid) else {
                return;
            };
            let Some(item_id) = inv.held_item().item_id else {
                return;
            };
            let Some(block_state) = basalt_world::block::item_to_default_block_state(item_id)
            else {
                return;
            };
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
            }
        }
    }

    // ── Helpers ────────────────────────────────────────────────────────

    /// Sends output to a player entity via their OutputHandle.
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
        }

        let ecs = basalt_ecs::Ecs::new();
        let game_loop = GameLoop::new(
            bus,
            world,
            chunk_cache,
            game_rx,
            io_tx,
            ecs,
            Vec::new(),
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
            inv.hotbar[0] = basalt_types::Slot {
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
        assert_eq!(inv.hotbar[0].item_id, Some(1));
        assert_eq!(inv.hotbar[0].item_count, 64);
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
        assert!(inv.hotbar[0].item_id.is_none());
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
}
