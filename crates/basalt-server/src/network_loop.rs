//! Network loop — dedicated OS thread for player-facing responsiveness.
//!
//! Runs at 20 TPS on a [`TickLoop`](crate::tick::TickLoop). Each tick:
//! 1. Drains the shared [`NetworkInput`] channel (movement, chat, commands, block acks)
//! 2. Drains the [`GameUpdate`] channel (cross-loop corrections)
//! 3. Dispatches events on the network [`EventBus`]
//! 4. Translates responses to [`ServerOutput`] packets
//!
//! This loop never performs heavy computation — it relays data and
//! updates player positions. Guaranteed <2ms per tick under normal load.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use basalt_api::context::{Response, ServerContext};
use basalt_api::events::{
    ChatMessageEvent, CommandEvent, PlayerJoinedEvent, PlayerLeftEvent, PlayerMovedEvent,
};
use basalt_api::{BroadcastMessage, EventBus, PlayerSnapshot};
use basalt_core::broadcast::ProfileProperty;
use basalt_protocol::packets::play::chat::{
    ClientboundPlayDeclareCommands, ClientboundPlaySystemChat,
};
use basalt_protocol::packets::play::entity::{
    ClientboundPlayEntityDestroy, ClientboundPlayEntityHeadRotation, ClientboundPlaySpawnEntity,
    ClientboundPlaySyncEntityPosition,
};
use basalt_protocol::packets::play::player::{
    ClientboundPlayGameStateChange, ClientboundPlayLogin, ClientboundPlayLoginSpawninfo,
    ClientboundPlayPlayerInfo, ClientboundPlayPlayerRemove, ClientboundPlayPosition,
};
use basalt_protocol::packets::play::world::{
    ClientboundPlayAcknowledgePlayerDigging, ClientboundPlayBlockChange,
    ClientboundPlayChunkBatchFinished, ClientboundPlayChunkBatchStart, ClientboundPlayMapChunk,
    ClientboundPlaySpawnPosition, ClientboundPlayUnloadChunk, ClientboundPlayUpdateViewPosition,
};
use basalt_types::{Encode, EncodedSize, Position, Uuid, VarInt, Vec3i16};
use tokio::sync::mpsc;

use crate::helpers::angle_to_byte;
use crate::messages::{NetworkInput, ServerOutput};
use crate::state::CommandMeta;

/// View distance radius in chunks.
const VIEW_RADIUS: i32 = 5;

/// Per-player state owned by the network loop.
struct NetworkPlayer {
    /// Server-assigned entity ID.
    entity_id: i32,
    /// Player UUID.
    uuid: Uuid,
    /// Player display name.
    username: String,
    /// Mojang skin texture data.
    skin_properties: Vec<ProfileProperty>,
    /// Current position.
    x: f64,
    y: f64,
    z: f64,
    /// Current look direction.
    yaw: f32,
    pitch: f32,
    /// Whether the player is on the ground.
    on_ground: bool,
    /// Chunks currently loaded by this player's client.
    loaded_chunks: HashSet<(i32, i32)>,
    /// Channel to send output packets to this player's net task.
    output_tx: mpsc::Sender<ServerOutput>,
}

/// The network loop state and logic.
///
/// Owns all per-player network state and the network event bus.
/// Runs as the callback inside a [`TickLoop`](crate::tick::TickLoop).
pub(crate) struct NetworkLoop {
    /// Per-player state, keyed by UUID.
    players: HashMap<Uuid, NetworkPlayer>,
    /// Network event bus (movement, chat, commands).
    bus: EventBus,
    /// Shared world for chunk reads.
    world: Arc<basalt_world::World>,
    /// Pre-built DeclareCommands packet payload.
    declare_commands: Vec<u8>,
    /// Command metadata for dispatching and /help.
    command_args: Vec<CommandMeta>,
    /// Receiver for net task → network loop messages.
    network_rx: mpsc::UnboundedReceiver<NetworkInput>,
}

impl NetworkLoop {
    /// Creates a new network loop with the given dependencies.
    pub fn new(
        bus: EventBus,
        world: Arc<basalt_world::World>,
        declare_commands: Vec<u8>,
        command_args: Vec<CommandMeta>,
        network_rx: mpsc::UnboundedReceiver<NetworkInput>,
    ) -> Self {
        Self {
            players: HashMap::new(),
            bus,
            world,
            declare_commands,
            command_args,
            network_rx,
        }
    }

    /// Processes one tick of the network loop.
    ///
    /// Called by the [`TickLoop`](crate::tick::TickLoop) at 20 TPS.
    /// Drains all pending input, dispatches events, and produces output.
    pub fn tick(&mut self, _tick: u64) {
        self.drain_network_input();
    }

    /// Drains all pending messages from net tasks.
    fn drain_network_input(&mut self) {
        while let Ok(msg) = self.network_rx.try_recv() {
            match msg {
                NetworkInput::PlayerConnected {
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
                NetworkInput::PlayerDisconnected {
                    uuid,
                    entity_id,
                    username,
                } => {
                    self.handle_player_disconnected(uuid, entity_id, &username);
                }
                NetworkInput::Position {
                    uuid,
                    x,
                    y,
                    z,
                    on_ground,
                } => {
                    self.handle_movement(uuid, Some((x, y, z)), None, on_ground);
                }
                NetworkInput::Look {
                    uuid,
                    yaw,
                    pitch,
                    on_ground,
                } => {
                    self.handle_movement(uuid, None, Some((yaw, pitch)), on_ground);
                }
                NetworkInput::PositionLook {
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
                NetworkInput::ChatMessage {
                    uuid,
                    username,
                    message,
                } => {
                    self.handle_chat(uuid, &username, &message);
                }
                NetworkInput::ChatCommand { uuid, command } => {
                    self.handle_command(uuid, &command);
                }
            }
        }
    }

    /// Handles a new player connection.
    #[allow(clippy::too_many_arguments)]
    fn handle_player_connected(
        &mut self,
        entity_id: i32,
        uuid: Uuid,
        username: String,
        skin_properties: Vec<ProfileProperty>,
        position: (f64, f64, f64),
        yaw: f32,
        pitch: f32,
        output_tx: mpsc::Sender<ServerOutput>,
    ) {
        // Send initial world data to the new player
        self.send_initial_world(entity_id, &username, position, &output_tx);

        // Send existing players' info to the new player
        let snapshot = PlayerSnapshot {
            username: username.clone(),
            uuid,
            entity_id,
            x: position.0,
            y: position.1,
            z: position.2,
            yaw,
            pitch,
            skin_properties: skin_properties.clone(),
        };

        // Send self info to the new player
        send_player_info_add(&output_tx, &snapshot);

        // Send all existing players to the new player
        for existing in self.players.values() {
            let existing_snapshot = PlayerSnapshot {
                username: existing.username.clone(),
                uuid: existing.uuid,
                entity_id: existing.entity_id,
                x: existing.x,
                y: existing.y,
                z: existing.z,
                yaw: existing.yaw,
                pitch: existing.pitch,
                skin_properties: existing.skin_properties.clone(),
            };
            send_player_info_add(&output_tx, &existing_snapshot);
            send_spawn_entity(&output_tx, &existing_snapshot);
        }

        // Send welcome message
        let welcome = basalt_types::TextComponent::text(format!("Welcome, {username}!")).color(
            basalt_types::TextColor::Named(basalt_types::NamedColor::Gold),
        );
        send_system_chat(&output_tx, &welcome, false);

        // Dispatch PlayerJoinedEvent
        let ctx = self.make_context(uuid, entity_id, &username, yaw, pitch);
        let mut event = PlayerJoinedEvent {
            info: snapshot.clone(),
        };
        self.bus.dispatch(&mut event, &ctx);
        self.process_responses(uuid, &ctx.drain_responses());

        // Notify existing players about the new player
        for existing in self.players.values() {
            send_player_info_add(&existing.output_tx, &snapshot);
            send_spawn_entity(&existing.output_tx, &snapshot);
            let join_msg = basalt_types::TextComponent::text(format!("{username} joined the game"))
                .color(basalt_types::TextColor::Named(
                    basalt_types::NamedColor::Yellow,
                ));
            send_system_chat(&existing.output_tx, &join_msg, false);
        }

        // Insert the player state
        let mut loaded_chunks = HashSet::new();
        let cx = (position.0 as i32) >> 4;
        let cz = (position.2 as i32) >> 4;
        for dx in -VIEW_RADIUS..=VIEW_RADIUS {
            for dz in -VIEW_RADIUS..=VIEW_RADIUS {
                loaded_chunks.insert((cx + dx, cz + dz));
            }
        }

        self.players.insert(
            uuid,
            NetworkPlayer {
                entity_id,
                uuid,
                username,
                skin_properties,
                x: position.0,
                y: position.1,
                z: position.2,
                yaw,
                pitch,
                on_ground: true,
                loaded_chunks,
                output_tx,
            },
        );
    }

    /// Sends the initial world data to a newly connected player.
    fn send_initial_world(
        &self,
        entity_id: i32,
        _username: &str,
        position: (f64, f64, f64),
        output_tx: &mpsc::Sender<ServerOutput>,
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
        let _ = output_tx.try_send(encode_packet(ClientboundPlayLogin::PACKET_ID, &login));

        // DeclareCommands
        if !self.declare_commands.is_empty() {
            let _ = output_tx.try_send(ServerOutput::SendPacket {
                id: ClientboundPlayDeclareCommands::PACKET_ID,
                data: self.declare_commands.clone(),
            });
        }

        // SpawnPosition
        let spawn_y = self.world.spawn_y() as i32;
        let spawn = ClientboundPlaySpawnPosition {
            location: Position::new(0, spawn_y, 0),
            angle: 0.0,
        };
        let _ = output_tx.try_send(encode_packet(
            ClientboundPlaySpawnPosition::PACKET_ID,
            &spawn,
        ));

        // GameEvent (wait for chunks)
        let game_event = ClientboundPlayGameStateChange {
            reason: 13,
            game_mode: 0.0,
        };
        let _ = output_tx.try_send(encode_packet(
            ClientboundPlayGameStateChange::PACKET_ID,
            &game_event,
        ));

        // UpdateViewPosition
        let cx = (position.0 as i32) >> 4;
        let cz = (position.2 as i32) >> 4;
        let view_pos = ClientboundPlayUpdateViewPosition {
            chunk_x: cx,
            chunk_z: cz,
        };
        let _ = output_tx.try_send(encode_packet(
            ClientboundPlayUpdateViewPosition::PACKET_ID,
            &view_pos,
        ));

        // Send chunks
        let _ = output_tx.try_send(encode_packet(
            ClientboundPlayChunkBatchStart::PACKET_ID,
            &ClientboundPlayChunkBatchStart,
        ));
        let mut count = 0i32;
        for dx in -VIEW_RADIUS..=VIEW_RADIUS {
            for dz in -VIEW_RADIUS..=VIEW_RADIUS {
                let packet = self.world.get_chunk_packet(cx + dx, cz + dz);
                let _ =
                    output_tx.try_send(encode_packet(ClientboundPlayMapChunk::PACKET_ID, &packet));
                count += 1;
            }
        }
        let _ = output_tx.try_send(encode_packet(
            ClientboundPlayChunkBatchFinished::PACKET_ID,
            &ClientboundPlayChunkBatchFinished { batch_size: count },
        ));

        // Position
        let pos = ClientboundPlayPosition {
            teleport_id: 1,
            x: position.0,
            y: position.1,
            z: position.2,
            dx: 0.0,
            dy: 0.0,
            dz: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            flags: 0,
        };
        let _ = output_tx.try_send(encode_packet(ClientboundPlayPosition::PACKET_ID, &pos));
    }

    /// Handles a player disconnection.
    fn handle_player_disconnected(&mut self, uuid: Uuid, entity_id: i32, username: &str) {
        self.players.remove(&uuid);

        // Dispatch PlayerLeftEvent
        let ctx = self.make_context(uuid, entity_id, username, 0.0, 0.0);
        let mut event = PlayerLeftEvent {
            uuid,
            entity_id,
            username: username.to_string(),
        };
        self.bus.dispatch(&mut event, &ctx);
        self.process_responses(uuid, &ctx.drain_responses());

        // Notify remaining players
        for player in self.players.values() {
            // Remove from tab list
            let _ = player.output_tx.try_send(encode_packet(
                ClientboundPlayPlayerRemove::PACKET_ID,
                &ClientboundPlayPlayerRemove {
                    players: vec![uuid],
                },
            ));
            // Destroy entity
            let _ = player.output_tx.try_send(encode_packet(
                ClientboundPlayEntityDestroy::PACKET_ID,
                &ClientboundPlayEntityDestroy {
                    entity_ids: vec![entity_id],
                },
            ));
            // Leave message
            let msg = basalt_types::TextComponent::text(format!("{username} left the game")).color(
                basalt_types::TextColor::Named(basalt_types::NamedColor::Yellow),
            );
            send_system_chat(&player.output_tx, &msg, false);
        }
    }

    /// Handles movement input (position and/or look update).
    fn handle_movement(
        &mut self,
        uuid: Uuid,
        pos: Option<(f64, f64, f64)>,
        look: Option<(f32, f32)>,
        on_ground: bool,
    ) {
        // Update player state (scoped to release the mutable borrow)
        let (entity_id, x, y, z, yaw, pitch, old_cx, old_cz, username) = {
            let Some(player) = self.players.get_mut(&uuid) else {
                return;
            };

            let old_cx = (player.x as i32) >> 4;
            let old_cz = (player.z as i32) >> 4;

            if let Some((x, y, z)) = pos {
                player.x = x;
                player.y = y;
                player.z = z;
            }
            if let Some((yaw, pitch)) = look {
                player.yaw = yaw;
                player.pitch = pitch;
            }
            player.on_ground = on_ground;

            (
                player.entity_id,
                player.x,
                player.y,
                player.z,
                player.yaw,
                player.pitch,
                old_cx,
                old_cz,
                player.username.clone(),
            )
        };

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

        // Process responses (chunk streaming, broadcasts)
        self.process_responses(uuid, &responses);

        // Broadcast movement to other players
        let sync = ClientboundPlaySyncEntityPosition {
            entity_id,
            x,
            y,
            z,
            dx: 0.0,
            dy: 0.0,
            dz: 0.0,
            yaw,
            pitch,
            on_ground,
        };
        let sync_data = encode_packet(ClientboundPlaySyncEntityPosition::PACKET_ID, &sync);
        let head = ClientboundPlayEntityHeadRotation {
            entity_id,
            head_yaw: angle_to_byte(yaw),
        };
        let head_data = encode_packet(ClientboundPlayEntityHeadRotation::PACKET_ID, &head);

        for other in self.players.values() {
            if other.uuid != uuid {
                let _ = other.output_tx.try_send(sync_data.clone());
                let _ = other.output_tx.try_send(head_data.clone());
            }
        }
    }

    /// Handles a chat message.
    fn handle_chat(&mut self, uuid: Uuid, username: &str, message: &str) {
        if message.len() > 256 {
            return;
        }

        let Some(player) = self.players.get(&uuid) else {
            return;
        };

        let ctx = self.make_context(uuid, player.entity_id, username, player.yaw, player.pitch);
        let mut event = ChatMessageEvent {
            username: username.to_string(),
            message: message.to_string(),
            cancelled: false,
        };
        self.bus.dispatch(&mut event, &ctx);
        let responses = ctx.drain_responses();
        self.process_responses(uuid, &responses);
    }

    /// Handles a slash command.
    fn handle_command(&mut self, uuid: Uuid, command: &str) {
        let Some(player) = self.players.get(&uuid) else {
            return;
        };

        let ctx = self.make_context(
            uuid,
            player.entity_id,
            &player.username.clone(),
            player.yaw,
            player.pitch,
        );
        ctx.set_command_list(
            self.command_args
                .iter()
                .map(|c| (c.name.clone(), c.description.clone()))
                .collect(),
        );
        let mut event = CommandEvent {
            command: command.to_string(),
            player_uuid: uuid,
            cancelled: false,
        };
        self.bus.dispatch(&mut event, &ctx);
        let responses = ctx.drain_responses();
        self.process_responses(uuid, &responses);
    }

    /// Processes event handler responses and sends output to players.
    fn process_responses(&mut self, source_uuid: Uuid, responses: &[Response]) {
        for response in responses {
            match response {
                Response::Broadcast(msg) => {
                    self.process_broadcast(source_uuid, msg);
                }
                Response::SendBlockAck { sequence } => {
                    if let Some(player) = self.players.get(&source_uuid) {
                        let _ = player.output_tx.try_send(encode_packet(
                            ClientboundPlayAcknowledgePlayerDigging::PACKET_ID,
                            &ClientboundPlayAcknowledgePlayerDigging {
                                sequence_id: *sequence,
                            },
                        ));
                    }
                }
                Response::SendSystemChat {
                    content,
                    action_bar,
                } => {
                    if let Some(player) = self.players.get(&source_uuid) {
                        let _ = player.output_tx.try_send(encode_packet(
                            ClientboundPlaySystemChat::PACKET_ID,
                            &ClientboundPlaySystemChat {
                                content: content.clone(),
                                is_action_bar: *action_bar,
                            },
                        ));
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
                    if let Some(player) = self.players.get_mut(&source_uuid) {
                        player.x = *x;
                        player.y = *y;
                        player.z = *z;
                        let _ = player.output_tx.try_send(encode_packet(
                            ClientboundPlayPosition::PACKET_ID,
                            &ClientboundPlayPosition {
                                teleport_id: *teleport_id,
                                x: *x,
                                y: *y,
                                z: *z,
                                dx: 0.0,
                                dy: 0.0,
                                dz: 0.0,
                                yaw: *yaw,
                                pitch: *pitch,
                                flags: 0,
                            },
                        ));
                    }
                }
                Response::StreamChunks { new_cx, new_cz } => {
                    self.stream_chunks(source_uuid, *new_cx, *new_cz);
                }
                Response::SendGameStateChange { reason, value } => {
                    if let Some(player) = self.players.get(&source_uuid) {
                        let _ = player.output_tx.try_send(encode_packet(
                            ClientboundPlayGameStateChange::PACKET_ID,
                            &ClientboundPlayGameStateChange {
                                reason: *reason,
                                game_mode: *value,
                            },
                        ));
                    }
                }
                // Persistence is handled by the game loop's I/O thread
                Response::PersistChunk { .. } => {}
            }
        }
    }

    /// Processes a broadcast message from an event handler.
    fn process_broadcast(&self, _source_uuid: Uuid, msg: &BroadcastMessage) {
        match msg {
            BroadcastMessage::Chat { content } => {
                let data = encode_packet(
                    ClientboundPlaySystemChat::PACKET_ID,
                    &ClientboundPlaySystemChat {
                        content: content.clone(),
                        is_action_bar: false,
                    },
                );
                for player in self.players.values() {
                    let _ = player.output_tx.try_send(data.clone());
                }
            }
            BroadcastMessage::PlayerJoined { .. }
            | BroadcastMessage::PlayerLeft { .. }
            | BroadcastMessage::EntityMoved { .. } => {
                // Handled directly by the loop, not through broadcast
            }
            BroadcastMessage::BlockChanged {
                x,
                y,
                z,
                block_state,
            } => {
                let data = encode_packet(
                    ClientboundPlayBlockChange::PACKET_ID,
                    &ClientboundPlayBlockChange {
                        location: Position::new(*x, *y, *z),
                        r#type: *block_state,
                    },
                );
                for player in self.players.values() {
                    let _ = player.output_tx.try_send(data.clone());
                }
            }
        }
    }

    /// Streams chunks when a player crosses a chunk boundary.
    fn stream_chunks(&mut self, uuid: Uuid, new_cx: i32, new_cz: i32) {
        let Some(player) = self.players.get_mut(&uuid) else {
            return;
        };

        // Update view center
        let _ = player.output_tx.try_send(encode_packet(
            ClientboundPlayUpdateViewPosition::PACKET_ID,
            &ClientboundPlayUpdateViewPosition {
                chunk_x: new_cx,
                chunk_z: new_cz,
            },
        ));

        let r = VIEW_RADIUS;

        // Compute new view set
        let mut in_view = HashSet::new();
        for dx in -r..=r {
            for dz in -r..=r {
                in_view.insert((new_cx + dx, new_cz + dz));
            }
        }

        // Unload chunks no longer in view
        let to_unload: Vec<(i32, i32)> = player
            .loaded_chunks
            .iter()
            .filter(|key| !in_view.contains(key))
            .copied()
            .collect();

        for (cx, cz) in &to_unload {
            let _ = player.output_tx.try_send(encode_packet(
                ClientboundPlayUnloadChunk::PACKET_ID,
                &ClientboundPlayUnloadChunk {
                    chunk_x: *cx,
                    chunk_z: *cz,
                },
            ));
            player.loaded_chunks.remove(&(*cx, *cz));
        }

        // Load new chunks
        let mut to_load = Vec::new();
        for &key in &in_view {
            if player.loaded_chunks.insert(key) {
                to_load.push(key);
            }
        }

        if !to_load.is_empty() {
            let _ = player.output_tx.try_send(encode_packet(
                ClientboundPlayChunkBatchStart::PACKET_ID,
                &ClientboundPlayChunkBatchStart,
            ));
            for (cx, cz) in &to_load {
                let packet = self.world.get_chunk_packet(*cx, *cz);
                let _ = player
                    .output_tx
                    .try_send(encode_packet(ClientboundPlayMapChunk::PACKET_ID, &packet));
            }
            let _ = player.output_tx.try_send(encode_packet(
                ClientboundPlayChunkBatchFinished::PACKET_ID,
                &ClientboundPlayChunkBatchFinished {
                    batch_size: to_load.len() as i32,
                },
            ));
        }
    }

    /// Creates a [`ServerContext`] for event dispatch.
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

/// Encodes a packet struct into a [`ServerOutput::SendPacket`].
fn encode_packet<P: Encode + EncodedSize>(packet_id: i32, packet: &P) -> ServerOutput {
    let mut data = Vec::with_capacity(packet.encoded_size());
    packet.encode(&mut data).expect("packet encoding failed");
    ServerOutput::SendPacket {
        id: packet_id,
        data,
    }
}

/// Sends a PlayerInfo "add player" packet via the output channel.
fn send_player_info_add(output_tx: &mpsc::Sender<ServerOutput>, info: &PlayerSnapshot) {
    let mut buf = Vec::new();

    // Action bitmask: bit 0 (add_player) | bit 2 (gamemode) | bit 3 (listed)
    let actions: u8 = 0x01 | 0x04 | 0x08;
    actions.encode(&mut buf).unwrap();
    VarInt(1).encode(&mut buf).unwrap();
    info.uuid.encode(&mut buf).unwrap();

    // Bit 0: add_player
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

    // Bit 2: gamemode (creative)
    VarInt(1).encode(&mut buf).unwrap();
    // Bit 3: listed
    true.encode(&mut buf).unwrap();

    let _ = output_tx.try_send(ServerOutput::SendPacket {
        id: ClientboundPlayPlayerInfo::PACKET_ID,
        data: buf,
    });
}

/// Sends a SpawnEntity packet for a player entity.
fn send_spawn_entity(output_tx: &mpsc::Sender<ServerOutput>, info: &PlayerSnapshot) {
    let packet = ClientboundPlaySpawnEntity {
        entity_id: info.entity_id,
        object_uuid: info.uuid,
        r#type: 147, // player entity type in 1.21.4
        x: info.x,
        y: info.y,
        z: info.z,
        pitch: angle_to_byte(info.pitch),
        yaw: (info.yaw / 360.0 * 256.0) as i8,
        head_pitch: 0,
        object_data: 0,
        velocity: Vec3i16 { x: 0, y: 0, z: 0 },
    };
    let _ = output_tx.try_send(encode_packet(
        ClientboundPlaySpawnEntity::PACKET_ID,
        &packet,
    ));
}

/// Sends a system chat message via the output channel.
fn send_system_chat(
    output_tx: &mpsc::Sender<ServerOutput>,
    component: &basalt_types::TextComponent,
    action_bar: bool,
) {
    let _ = output_tx.try_send(encode_packet(
        ClientboundPlaySystemChat::PACKET_ID,
        &ClientboundPlaySystemChat {
            content: component.to_nbt(),
            is_action_bar: action_bar,
        },
    ));
}
