//! Play state loop with packet dispatch and multi-player broadcast.
//!
//! Handles the main gameplay loop: sends initial world data (login,
//! chunks, position), then enters a read loop that dispatches incoming
//! packets, sends periodic keep-alive probes, and processes broadcast
//! messages from other players (chat, join/leave, movement).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use basalt_net::connection::{Connection, Play};
use basalt_protocol::packets::play::ServerboundPlayPacket;
use basalt_protocol::packets::play::chat::{
    ClientboundPlayDeclareCommands, ClientboundPlaySystemChat, ClientboundPlayTabComplete,
    ClientboundPlayTabCompleteMatches, ServerboundPlayTabComplete,
};
use basalt_protocol::packets::play::entity::{
    ClientboundPlayEntityDestroy, ClientboundPlayEntityHeadRotation, ClientboundPlaySpawnEntity,
    ClientboundPlaySyncEntityPosition,
};
use basalt_protocol::packets::play::misc::ClientboundPlayKeepAlive;
use basalt_protocol::packets::play::player::{
    ClientboundPlayGameStateChange, ClientboundPlayLogin, ClientboundPlayLoginSpawninfo,
    ClientboundPlayPlayerInfo, ClientboundPlayPlayerRemove, ClientboundPlayPosition,
};
use basalt_protocol::packets::play::world::{
    ClientboundPlayAcknowledgePlayerDigging, ClientboundPlayBlockChange,
    ClientboundPlayChunkBatchFinished, ClientboundPlayChunkBatchStart, ClientboundPlayMapChunk,
    ClientboundPlaySpawnPosition, ClientboundPlayUnloadChunk, ClientboundPlayUpdateViewPosition,
};
use basalt_types::{Encode, Position, VarInt, Vec3i16};
use tokio::sync::broadcast;

use basalt_api::context::{Response, ServerContext};
use basalt_api::events::{
    BlockBrokenEvent, BlockPlacedEvent, ChatMessageEvent, CommandEvent, PlayerMovedEvent,
};
use basalt_api::{BroadcastMessage, Event, PlayerSnapshot};

use crate::helpers::{RawPayload, angle_to_byte};
use crate::player::PlayerState;
use crate::state::ServerState;

/// Sends the initial world data to the client and enters the play loop.
pub(crate) async fn run_play_loop(
    mut conn: Connection<Play>,
    addr: SocketAddr,
    player: &mut PlayerState,
    state: &Arc<ServerState>,
    rx: broadcast::Receiver<BroadcastMessage>,
    existing_players: &[PlayerSnapshot],
) -> crate::error::Result<()> {
    send_initial_world(&mut conn, addr, player, state).await?;

    // Send the player's own PlayerInfo so they appear in their own Tab list
    let self_snapshot = PlayerSnapshot {
        username: player.username.clone(),
        uuid: player.uuid,
        entity_id: player.entity_id,
        x: player.x,
        y: player.y,
        z: player.z,
        yaw: player.yaw,
        pitch: player.pitch,
        skin_properties: player.skin_properties.clone(),
    };
    send_player_info_add(&mut conn, &self_snapshot).await?;

    // Send PlayerInfo + SpawnEntity for all existing players
    for existing in existing_players {
        send_player_info_add(&mut conn, existing).await?;
        send_spawn_entity(&mut conn, existing).await?;
    }

    crate::chat::send_welcome(&mut conn, &player.username).await?;

    log::info!(target: "basalt::play", "[{addr}] {} joined, starting play loop", player.username);

    play_loop(&mut conn, addr, player, state, rx).await
}

/// View distance radius in chunks. Determines how many chunks are
/// sent around the player. Total chunks = (2*RADIUS+1)^2.
const VIEW_RADIUS: i32 = 5;

/// Sends the initial world data that the client needs to enter the game.
async fn send_initial_world(
    conn: &mut Connection<Play>,
    addr: SocketAddr,
    player: &mut PlayerState,
    state: &Arc<ServerState>,
) -> crate::error::Result<()> {
    let login = ClientboundPlayLogin {
        entity_id: player.entity_id,
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
    conn.write_packet_typed(ClientboundPlayLogin::PACKET_ID, &login)
        .await?;
    log::debug!(target: "basalt::play", "[{addr}] -> Login (Play)");

    // DeclareCommands must be sent right after Login, before chunks
    send_declare_commands(conn, addr, state).await?;

    let spawn_y = state.world.spawn_y() as i32;
    let spawn = ClientboundPlaySpawnPosition {
        location: Position::new(0, spawn_y, 0),
        angle: 0.0,
    };
    conn.write_packet_typed(ClientboundPlaySpawnPosition::PACKET_ID, &spawn)
        .await?;
    log::debug!(target: "basalt::play", "[{addr}] -> SpawnPosition");

    let game_event = ClientboundPlayGameStateChange {
        reason: 13,
        game_mode: 0.0,
    };
    conn.write_packet_typed(ClientboundPlayGameStateChange::PACKET_ID, &game_event)
        .await?;
    log::debug!(target: "basalt::play", "[{addr}] -> GameEvent (wait for chunks)");

    // Tell the client where to center its chunk rendering
    let spawn_cx = (player.x as i32) >> 4;
    let spawn_cz = (player.z as i32) >> 4;
    let view_pos = ClientboundPlayUpdateViewPosition {
        chunk_x: spawn_cx,
        chunk_z: spawn_cz,
    };
    conn.write_packet_typed(ClientboundPlayUpdateViewPosition::PACKET_ID, &view_pos)
        .await?;
    let chunk_count =
        send_chunks_around(conn, state, player, spawn_cx, spawn_cz, VIEW_RADIUS).await?;
    log::debug!(target: "basalt::play", "[{addr}] -> {chunk_count} chunks (radius {VIEW_RADIUS})");

    let position = ClientboundPlayPosition {
        teleport_id: 1,
        x: player.x,
        y: player.y,
        z: player.z,
        dx: 0.0,
        dy: 0.0,
        dz: 0.0,
        yaw: 0.0,
        pitch: 0.0,
        flags: 0,
    };
    conn.write_packet_typed(ClientboundPlayPosition::PACKET_ID, &position)
        .await?;
    log::debug!(target: "basalt::play", "[{addr}] -> Position ({}, {}, {})", player.x, player.y, player.z);

    Ok(())
}

/// Sends the DeclareCommands packet for client tab-completion.
///
/// Skipped if no commands are registered (command plugin disabled).
async fn send_declare_commands(
    conn: &mut Connection<Play>,
    addr: SocketAddr,
    state: &Arc<ServerState>,
) -> crate::error::Result<()> {
    if state.declare_commands.is_empty() {
        return Ok(());
    }
    conn.write_packet_typed(
        ClientboundPlayDeclareCommands::PACKET_ID,
        &RawPayload(state.declare_commands.clone()),
    )
    .await?;
    log::debug!(target: "basalt::play", "[{addr}] -> DeclareCommands");
    Ok(())
}

/// Handles a serverbound TabComplete request.
///
/// Finds the command being typed and the current argument position,
/// then sends matching suggestions from Options or Player arg types.
async fn handle_tab_complete(
    conn: &mut Connection<Play>,
    state: &Arc<ServerState>,
    tc: &ServerboundPlayTabComplete,
) -> crate::error::Result<()> {
    use basalt_command::Arg;

    let text = tc.text.trim_start_matches('/');
    let parts: Vec<&str> = text.split_whitespace().collect();

    let cmd_name = parts.first().copied().unwrap_or("");
    let arg_index = if text.ends_with(' ') {
        parts.len().saturating_sub(1)
    } else {
        parts.len().saturating_sub(2)
    };
    let prefix = if text.ends_with(' ') {
        ""
    } else {
        parts.last().copied().unwrap_or("")
    };

    let mut suggestions = Vec::new();

    if let Some(meta) = state.command_args.iter().find(|c| c.name == cmd_name) {
        // Get the arg lists to search
        let arg_lists: Vec<&Vec<basalt_command::CommandArg>> = if !meta.variants.is_empty() {
            meta.variants.iter().collect()
        } else {
            vec![&meta.args]
        };

        for arg_list in &arg_lists {
            if let Some(arg_def) = arg_list.get(arg_index) {
                match &arg_def.arg_type {
                    Arg::Options(choices) => {
                        for choice in choices {
                            if choice.starts_with(prefix) {
                                suggestions.push(ClientboundPlayTabCompleteMatches {
                                    r#match: choice.clone(),
                                    tooltip: None,
                                });
                            }
                        }
                    }
                    // Entity/Player/GameProfile — client handles natively
                    Arg::Boolean => {
                        for val in &["true", "false"] {
                            if val.starts_with(prefix) {
                                suggestions.push(ClientboundPlayTabCompleteMatches {
                                    r#match: (*val).to_string(),
                                    tooltip: None,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if !suggestions.is_empty() {
        let start = (tc.text.len() - prefix.len()) as i32;
        let response = ClientboundPlayTabComplete {
            transaction_id: tc.transaction_id,
            start,
            length: prefix.len() as i32,
            matches: suggestions,
        };
        conn.write_packet_typed(ClientboundPlayTabComplete::PACKET_ID, &response)
            .await?;
    }

    Ok(())
}

/// Main play loop with three concurrent branches:
/// 1. Keep-alive timer
/// 2. Client packet reader
/// 3. Broadcast message receiver
async fn play_loop(
    conn: &mut Connection<Play>,
    addr: SocketAddr,
    player: &mut PlayerState,
    state: &Arc<ServerState>,
    mut rx: broadcast::Receiver<BroadcastMessage>,
) -> crate::error::Result<()> {
    let mut keep_alive = tokio::time::interval(std::time::Duration::from_secs(15));
    keep_alive.tick().await;

    loop {
        tokio::select! {
            _ = keep_alive.tick() => {
                // Disconnect if the client hasn't responded to the previous keep-alive
                if player.last_keep_alive_id > 0
                    && player.last_keep_alive_sent.elapsed() > std::time::Duration::from_secs(30)
                {
                    log::warn!(target: "basalt::play", "[{addr}] {} timed out (no keep-alive response in 30s)", player.username);
                    break;
                }
                player.last_keep_alive_id += 1;
                player.last_keep_alive_sent = Instant::now();
                let ka = ClientboundPlayKeepAlive {
                    keep_alive_id: player.last_keep_alive_id,
                };
                conn.write_packet_typed(ClientboundPlayKeepAlive::PACKET_ID, &ka).await?;
            }
            result = conn.read_packet() => {
                match result {
                    Ok(packet) => {
                        // Handle TabComplete inline (not an event)
                        if let ServerboundPlayPacket::TabComplete(tc) = &packet {
                            handle_tab_complete(conn, state, tc).await?;
                            continue;
                        }
                        if let Some(mut event) = packet_to_event(addr, player, packet) {
                            let ctx = ServerContext::new(
                                Arc::clone(&state.world),
                                player.uuid,
                                player.entity_id,
                                player.username.clone(),
                            );
                            ctx.set_command_list(
                                state.command_args.iter()
                                    .map(|c| (c.name.clone(), c.description.clone()))
                                    .collect(),
                            );
                            state.event_bus.dispatch_dyn(&mut *event, &ctx);
                            execute_responses(conn, state, player, &ctx.drain_responses()).await?;
                        }
                    }
                    Err(basalt_net::Error::Protocol(
                        basalt_protocol::Error::UnknownPacket { id, .. }
                    )) => {
                        // Common packets (settings, plugin channels) are
                        // skipped by the codegen and produce UnknownPacket.
                        // Ignore them silently.
                        log::trace!(target: "basalt::play", "[{addr}] {} unknown packet 0x{id:02x}", player.username);
                    }
                    Err(e) => {
                        log::info!(target: "basalt::play", "[{addr}] {} disconnected: {e}", player.username);
                        break;
                    }
                }
            }
            result = rx.recv() => {
                match result {
                    Ok(msg) => handle_broadcast(conn, player, msg).await?,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        log::warn!(target: "basalt::play", "[{addr}] {} missed {n} broadcast messages", player.username);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    Ok(())
}

/// Converts a serverbound packet into a game event, if applicable.
///
/// Updates player state synchronously (position, look, inventory),
/// then constructs a boxed event for dispatch through the event bus.
/// Returns `None` for packets that are fully handled inline
/// (keep-alive, teleport confirm, inventory updates, etc.).
fn packet_to_event(
    addr: SocketAddr,
    player: &mut PlayerState,
    packet: ServerboundPlayPacket,
) -> Option<Box<dyn Event>> {
    match packet {
        ServerboundPlayPacket::KeepAlive(ka) => {
            if ka.keep_alive_id == player.last_keep_alive_id {
                let rtt = player.last_keep_alive_sent.elapsed();
                log::trace!(target: "basalt::play", "[{addr}] {} keep-alive OK (RTT: {}ms)", player.username, rtt.as_millis());
            } else {
                log::warn!(target: "basalt::play", "[{addr}] {} keep-alive mismatch: expected {}, got {}", player.username, player.last_keep_alive_id, ka.keep_alive_id);
            }
            None
        }
        ServerboundPlayPacket::TeleportConfirm(tc) => {
            log::trace!(target: "basalt::play", "[{addr}] {} teleport confirmed (id={})", player.username, tc.teleport_id);
            player.teleport_confirmed = true;
            None
        }
        ServerboundPlayPacket::PlayerLoaded(_) => {
            log::debug!(target: "basalt::play", "[{addr}] {} finished loading", player.username);
            player.loaded = true;
            None
        }
        ServerboundPlayPacket::Position(p) => {
            if !is_valid_position(p.x, p.y, p.z) {
                log::warn!(target: "basalt::play", "[{addr}] {} sent invalid position ({}, {}, {})", player.username, p.x, p.y, p.z);
                return None;
            }
            let old_cx = (player.x as i32) >> 4;
            let old_cz = (player.z as i32) >> 4;
            player.update_position(p.x, p.y, p.z);
            player.update_on_ground(p.flags);
            Some(Box::new(PlayerMovedEvent {
                entity_id: player.entity_id,
                x: player.x,
                y: player.y,
                z: player.z,
                yaw: player.yaw,
                pitch: player.pitch,
                on_ground: player.on_ground,
                old_cx,
                old_cz,
            }))
        }
        ServerboundPlayPacket::PositionLook(p) => {
            if !is_valid_position(p.x, p.y, p.z) {
                log::warn!(target: "basalt::play", "[{addr}] {} sent invalid position ({}, {}, {})", player.username, p.x, p.y, p.z);
                return None;
            }
            let old_cx = (player.x as i32) >> 4;
            let old_cz = (player.z as i32) >> 4;
            player.update_position(p.x, p.y, p.z);
            player.update_look(p.yaw, p.pitch);
            player.update_on_ground(p.flags);
            Some(Box::new(PlayerMovedEvent {
                entity_id: player.entity_id,
                x: player.x,
                y: player.y,
                z: player.z,
                yaw: player.yaw,
                pitch: player.pitch,
                on_ground: player.on_ground,
                old_cx,
                old_cz,
            }))
        }
        ServerboundPlayPacket::Look(p) => {
            let old_cx = (player.x as i32) >> 4;
            let old_cz = (player.z as i32) >> 4;
            player.update_look(p.yaw, p.pitch);
            player.update_on_ground(p.flags);
            Some(Box::new(PlayerMovedEvent {
                entity_id: player.entity_id,
                x: player.x,
                y: player.y,
                z: player.z,
                yaw: player.yaw,
                pitch: player.pitch,
                on_ground: player.on_ground,
                old_cx,
                old_cz,
            }))
        }
        ServerboundPlayPacket::Flying(p) => {
            player.update_on_ground(p.flags);
            None
        }
        ServerboundPlayPacket::ChatMessage(msg) => {
            if msg.message.len() > 256 {
                log::warn!(target: "basalt::play", "[{addr}] {} sent oversized chat message ({} bytes)", player.username, msg.message.len());
                return None;
            }
            log::info!(target: "basalt::play", "[{addr}] <{}> {}", player.username, msg.message);
            Some(Box::new(ChatMessageEvent {
                username: player.username.clone(),
                message: msg.message,
                cancelled: false,
            }))
        }
        ServerboundPlayPacket::ChatCommand(cmd) => {
            log::info!(target: "basalt::play", "[{addr}] {} issued /{}", player.username, cmd.command);
            Some(Box::new(CommandEvent {
                command: cmd.command,
                player_uuid: player.uuid,
                cancelled: false,
            }))
        }
        ServerboundPlayPacket::BlockDig(dig) => {
            let pos = dig.location;
            if dig.status == 0 {
                log::debug!(target: "basalt::play", "[{addr}] {} broke block ({}, {}, {})", player.username, pos.x, pos.y, pos.z);
                Some(Box::new(BlockBrokenEvent {
                    x: pos.x,
                    y: pos.y,
                    z: pos.z,
                    sequence: dig.sequence,
                    player_uuid: player.uuid,
                    cancelled: false,
                }))
            } else {
                None
            }
        }
        ServerboundPlayPacket::BlockPlace(place) => {
            let target = place.location;
            let (dx, dy, dz) = face_offset(place.direction);
            let (px, py, pz) = (target.x + dx, target.y + dy, target.z + dz);

            let item = player.held_item();
            if let Some(item_id) = item.item_id
                && let Some(block_state) = basalt_world::block::item_to_default_block_state(item_id)
            {
                log::debug!(target: "basalt::play", "[{addr}] {} placed block ({px}, {py}, {pz}) state={block_state}", player.username);
                Some(Box::new(BlockPlacedEvent {
                    x: px,
                    y: py,
                    z: pz,
                    block_state,
                    sequence: place.sequence,
                    player_uuid: player.uuid,
                    cancelled: false,
                }))
            } else {
                None
            }
        }
        ServerboundPlayPacket::HeldItemSlot(slot) => {
            let idx = slot.slot_id as u8;
            if idx < 9 {
                player.held_slot = idx;
            }
            None
        }
        ServerboundPlayPacket::SetCreativeSlot(creative) => {
            player.set_creative_slot(creative.slot, creative.item);
            None
        }
        ServerboundPlayPacket::CustomPayload(_)
        | ServerboundPlayPacket::PlayerInput(_)
        | ServerboundPlayPacket::TickEnd(_)
        | ServerboundPlayPacket::ChunkBatchReceived(_)
        | ServerboundPlayPacket::Pong(_)
        | ServerboundPlayPacket::MessageAcknowledgement(_)
        | ServerboundPlayPacket::ConfigurationAcknowledged(_)
        | ServerboundPlayPacket::UseItem(_)
        | ServerboundPlayPacket::ArmAnimation(_) => None,
        other => {
            log::trace!(target: "basalt::play", "[{addr}] {} unhandled packet: {:?}", player.username, std::mem::discriminant(&other));
            None
        }
    }
}

/// Maximum valid coordinate magnitude in Minecraft.
const MAX_COORDINATE: f64 = 30_000_000.0;

/// Validates that coordinates are finite and within the Minecraft world bounds.
fn is_valid_position(x: f64, y: f64, z: f64) -> bool {
    x.is_finite()
        && y.is_finite()
        && z.is_finite()
        && x.abs() <= MAX_COORDINATE
        && z.abs() <= MAX_COORDINATE
}

/// Executes queued responses from event handlers.
///
/// This is the async boundary — handlers are sync, but responses
/// produce async packet writes and chunk streaming.
async fn execute_responses(
    conn: &mut Connection<Play>,
    state: &Arc<ServerState>,
    player: &mut PlayerState,
    responses: &[Response],
) -> crate::error::Result<()> {
    for response in responses {
        match response {
            Response::Broadcast(msg) => {
                state.broadcast(msg.clone());
            }
            Response::SendBlockAck { sequence } => {
                send_block_ack(conn, *sequence).await?;
            }
            Response::SendSystemChat {
                content,
                action_bar,
            } => {
                let packet = ClientboundPlaySystemChat {
                    content: content.clone(),
                    is_action_bar: *action_bar,
                };
                conn.write_packet_typed(ClientboundPlaySystemChat::PACKET_ID, &packet)
                    .await?;
            }
            Response::SendPosition {
                teleport_id,
                x,
                y,
                z,
                yaw,
                pitch,
            } => {
                player.update_position(*x, *y, *z);
                player.teleport_confirmed = false;
                let packet = ClientboundPlayPosition {
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
                };
                conn.write_packet_typed(ClientboundPlayPosition::PACKET_ID, &packet)
                    .await?;
            }
            Response::StreamChunks { new_cx, new_cz } => {
                stream_chunks(conn, state, player, *new_cx, *new_cz).await?;
            }
            Response::SendGameStateChange { reason, value } => {
                let packet = ClientboundPlayGameStateChange {
                    reason: *reason,
                    game_mode: *value,
                };
                conn.write_packet_typed(ClientboundPlayGameStateChange::PACKET_ID, &packet)
                    .await?;
            }
        }
    }
    Ok(())
}

/// Handles an incoming broadcast message from another player.
async fn handle_broadcast(
    conn: &mut Connection<Play>,
    player: &PlayerState,
    msg: BroadcastMessage,
) -> crate::error::Result<()> {
    match msg {
        BroadcastMessage::Chat { content } => {
            let packet = ClientboundPlaySystemChat {
                content,
                is_action_bar: false,
            };
            conn.write_packet_typed(ClientboundPlaySystemChat::PACKET_ID, &packet)
                .await?;
        }
        BroadcastMessage::PlayerJoined { info } => {
            // Skip our own join message
            if info.uuid == player.uuid {
                return Ok(());
            }
            send_player_info_add(conn, &info).await?;
            send_spawn_entity(conn, &info).await?;

            // Send join message
            let msg =
                basalt_types::TextComponent::text(format!("{} joined the game", info.username))
                    .color(basalt_types::TextColor::Named(
                        basalt_types::NamedColor::Yellow,
                    ));
            crate::chat::send_system_message(conn, &msg, false).await?;
        }
        BroadcastMessage::PlayerLeft {
            uuid,
            entity_id,
            username,
        } => {
            // Remove from tab list
            let remove = ClientboundPlayPlayerRemove {
                players: vec![uuid],
            };
            conn.write_packet_typed(ClientboundPlayPlayerRemove::PACKET_ID, &remove)
                .await?;

            // Destroy entity
            let destroy = ClientboundPlayEntityDestroy {
                entity_ids: vec![entity_id],
            };
            conn.write_packet_typed(ClientboundPlayEntityDestroy::PACKET_ID, &destroy)
                .await?;

            // Leave message
            let msg = basalt_types::TextComponent::text(format!("{username} left the game")).color(
                basalt_types::TextColor::Named(basalt_types::NamedColor::Yellow),
            );
            crate::chat::send_system_message(conn, &msg, false).await?;
        }
        BroadcastMessage::EntityMoved {
            entity_id,
            x,
            y,
            z,
            yaw,
            pitch,
            on_ground,
        } => {
            // Skip our own entity
            if entity_id == player.entity_id {
                return Ok(());
            }
            // Use sync_entity_position which includes velocity deltas
            // and uses f32 angles (correct for 1.21.4).
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
            conn.write_packet_typed(ClientboundPlaySyncEntityPosition::PACKET_ID, &sync)
                .await?;

            // Head rotation is separate from body — the client renders
            // them independently. The angle is encoded as 256 = 360°.
            let head = ClientboundPlayEntityHeadRotation {
                entity_id,
                head_yaw: angle_to_byte(yaw),
            };
            conn.write_packet_typed(ClientboundPlayEntityHeadRotation::PACKET_ID, &head)
                .await?;
        }
        BroadcastMessage::BlockChanged {
            x,
            y,
            z,
            block_state,
        } => {
            let packet = ClientboundPlayBlockChange {
                location: Position::new(x, y, z),
                r#type: block_state,
            };
            conn.write_packet_typed(ClientboundPlayBlockChange::PACKET_ID, &packet)
                .await?;
        }
    }
    Ok(())
}

/// Sends a PlayerInfo "add player" packet for the given player.
///
/// The PlayerInfo packet uses a bitmask for actions and only includes
/// data for active bits on the wire. The generated struct can't handle
/// this (it encodes all fields including empty ones with VarInt length
/// prefixes), so we build the raw payload manually.
async fn send_player_info_add(
    conn: &mut Connection<Play>,
    info: &PlayerSnapshot,
) -> crate::error::Result<()> {
    let mut buf = Vec::new();

    // Action bitmask: bit 0 (add_player) | bit 2 (gamemode) | bit 3 (listed)
    let actions: u8 = 0x01 | 0x04 | 0x08;
    actions.encode(&mut buf).unwrap();

    // Number of entries
    VarInt(1).encode(&mut buf).unwrap();

    // Entry UUID
    info.uuid.encode(&mut buf).unwrap();

    // Bit 0 data — add_player: name (String) + properties
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

    // Bit 1 not set — no chat_session data

    // Bit 2 data — gamemode: VarInt (1 = creative)
    VarInt(1).encode(&mut buf).unwrap();

    // Bit 3 data — listed: bool (true)
    true.encode(&mut buf).unwrap();

    // Bits 4-7 not set — no latency, display_name, list_priority, show_hat

    // Use a RawPayload wrapper since write_packet is private
    // and write_packet_typed requires Encode + EncodedSize.
    conn.write_packet_typed(ClientboundPlayPlayerInfo::PACKET_ID, &RawPayload(buf))
        .await?;
    Ok(())
}

/// Sends a SpawnEntity packet for a player entity.
///
/// Player entities use type ID 128 in Minecraft 1.21.4.
async fn send_spawn_entity(
    conn: &mut Connection<Play>,
    info: &PlayerSnapshot,
) -> crate::error::Result<()> {
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
    conn.write_packet_typed(ClientboundPlaySpawnEntity::PACKET_ID, &packet)
        .await?;
    Ok(())
}

/// Returns the (dx, dy, dz) offset for a block face direction.
///
/// Block faces in the Minecraft protocol:
/// 0 = bottom (-Y), 1 = top (+Y), 2 = north (-Z),
/// 3 = south (+Z), 4 = west (-X), 5 = east (+X).
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

/// Sends a block action acknowledgement to the client.
///
/// The client waits for this before applying block predictions.
/// The sequence ID matches the one sent in the dig/place packet.
async fn send_block_ack(conn: &mut Connection<Play>, sequence: i32) -> crate::error::Result<()> {
    let ack = ClientboundPlayAcknowledgePlayerDigging {
        sequence_id: sequence,
    };
    conn.write_packet_typed(ClientboundPlayAcknowledgePlayerDigging::PACKET_ID, &ack)
        .await?;
    Ok(())
}

/// Sends a batch of chunks in a radius around (cx, cz).
///
/// Returns the number of chunks sent. Each chunk is generated
/// by the world if not already cached.
async fn send_chunks_around(
    conn: &mut Connection<Play>,
    state: &Arc<ServerState>,
    player: &mut PlayerState,
    cx: i32,
    cz: i32,
    radius: i32,
) -> crate::error::Result<i32> {
    conn.write_packet_typed(
        ClientboundPlayChunkBatchStart::PACKET_ID,
        &ClientboundPlayChunkBatchStart,
    )
    .await?;

    let mut count = 0;
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            let key = (cx + dx, cz + dz);
            if player.loaded_chunks.insert(key) {
                // Not previously sent — send it
                let packet = state.world.get_chunk_packet(key.0, key.1);
                conn.write_packet_typed(ClientboundPlayMapChunk::PACKET_ID, &packet)
                    .await?;
                count += 1;
            }
        }
    }

    conn.write_packet_typed(
        ClientboundPlayChunkBatchFinished::PACKET_ID,
        &ClientboundPlayChunkBatchFinished { batch_size: count },
    )
    .await?;

    Ok(count)
}

/// Streams chunks when the player crosses a chunk boundary.
///
/// Sends new chunks that entered the view distance and unloads
/// chunks that left it. Only the difference is sent, not the
/// entire view.
async fn stream_chunks(
    conn: &mut Connection<Play>,
    state: &Arc<ServerState>,
    player: &mut PlayerState,
    new_cx: i32,
    new_cz: i32,
) -> crate::error::Result<()> {
    // Update the client's view center so it knows which chunks to render
    let view_pos = ClientboundPlayUpdateViewPosition {
        chunk_x: new_cx,
        chunk_z: new_cz,
    };
    conn.write_packet_typed(ClientboundPlayUpdateViewPosition::PACKET_ID, &view_pos)
        .await?;

    let r = VIEW_RADIUS;

    // Collect chunks that should be in view
    let mut in_view = std::collections::HashSet::new();
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
        let packet = ClientboundPlayUnloadChunk {
            chunk_x: *cx,
            chunk_z: *cz,
        };
        conn.write_packet_typed(ClientboundPlayUnloadChunk::PACKET_ID, &packet)
            .await?;
        player.loaded_chunks.remove(&(*cx, *cz));
    }

    // Load new chunks — only those not already sent to this player
    let mut to_load = Vec::new();
    for &key in &in_view {
        if player.loaded_chunks.insert(key) {
            to_load.push(key);
        }
    }

    if !to_load.is_empty() {
        conn.write_packet_typed(
            ClientboundPlayChunkBatchStart::PACKET_ID,
            &ClientboundPlayChunkBatchStart,
        )
        .await?;

        for (cx, cz) in &to_load {
            let packet = state.world.get_chunk_packet(*cx, *cz);
            conn.write_packet_typed(ClientboundPlayMapChunk::PACKET_ID, &packet)
                .await?;
        }

        conn.write_packet_typed(
            ClientboundPlayChunkBatchFinished::PACKET_ID,
            &ClientboundPlayChunkBatchFinished {
                batch_size: to_load.len() as i32,
            },
        )
        .await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use basalt_protocol::packets::play::misc::ServerboundPlayKeepAlive;
    use basalt_protocol::packets::play::player::{
        ServerboundPlayFlying, ServerboundPlayLook, ServerboundPlayPosition,
        ServerboundPlayPositionLook, ServerboundPlayTeleportConfirm,
    };
    use basalt_types::Uuid;

    fn test_player() -> PlayerState {
        PlayerState::new("Steve".into(), Uuid::default(), 1, vec![])
    }

    fn test_addr() -> SocketAddr {
        "127.0.0.1:12345".parse().unwrap()
    }

    #[test]
    fn teleport_confirm_returns_none() {
        let mut player = test_player();
        assert!(!player.teleport_confirmed);

        let event = packet_to_event(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::TeleportConfirm(ServerboundPlayTeleportConfirm {
                teleport_id: 1,
            }),
        );

        assert!(player.teleport_confirmed);
        assert!(event.is_none());
    }

    #[test]
    fn player_loaded_returns_none() {
        let mut player = test_player();
        assert!(!player.loaded);

        let event = packet_to_event(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::PlayerLoaded(
                basalt_protocol::packets::play::player::ServerboundPlayPlayerLoaded,
            ),
        );

        assert!(player.loaded);
        assert!(event.is_none());
    }

    #[test]
    fn keep_alive_returns_none() {
        let mut player = test_player();
        player.last_keep_alive_id = 42;

        let event = packet_to_event(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::KeepAlive(ServerboundPlayKeepAlive { keep_alive_id: 42 }),
        );

        assert!(event.is_none());
    }

    #[test]
    fn position_returns_moved_event() {
        let mut player = test_player();

        let event = packet_to_event(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::Position(ServerboundPlayPosition {
                x: 10.5,
                y: 64.0,
                z: -30.2,
                flags: 0x01,
            }),
        );

        assert_eq!(player.x, 10.5);
        assert_eq!(player.y, 64.0);
        assert_eq!(player.z, -30.2);
        assert!(player.on_ground);
        let event = event.unwrap();
        let moved = event.as_any().downcast_ref::<PlayerMovedEvent>().unwrap();
        assert_eq!(moved.x, 10.5);
    }

    #[test]
    fn position_look_returns_moved_event() {
        let mut player = test_player();

        let event = packet_to_event(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::PositionLook(ServerboundPlayPositionLook {
                x: 5.0,
                y: 100.0,
                z: 5.0,
                yaw: 90.0,
                pitch: -45.0,
                flags: 0x00,
            }),
        );

        assert_eq!(player.x, 5.0);
        assert_eq!(player.yaw, 90.0);
        assert!(event.unwrap().as_any().is::<PlayerMovedEvent>());
    }

    #[test]
    fn look_returns_moved_event() {
        let mut player = test_player();

        let event = packet_to_event(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::Look(ServerboundPlayLook {
                yaw: 180.0,
                pitch: 0.0,
                flags: 0x01,
            }),
        );

        assert_eq!(player.yaw, 180.0);
        assert!(event.unwrap().as_any().is::<PlayerMovedEvent>());
    }

    #[test]
    fn flying_returns_none() {
        let mut player = test_player();

        let event = packet_to_event(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::Flying(ServerboundPlayFlying { flags: 0x01 }),
        );

        assert!(player.on_ground);
        assert!(event.is_none());
    }

    #[test]
    fn chat_returns_event() {
        let mut player = test_player();

        let event = packet_to_event(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::ChatMessage(
                basalt_protocol::packets::play::chat::ServerboundPlayChatMessage {
                    message: "hello".into(),
                    timestamp: 0,
                    salt: 0,
                    signature: None,
                    offset: 0,
                    acknowledged: vec![],
                },
            ),
        );

        assert!(event.unwrap().as_any().is::<ChatMessageEvent>());
    }

    #[test]
    fn command_returns_event() {
        let mut player = test_player();

        let event = packet_to_event(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::ChatCommand(
                basalt_protocol::packets::play::chat::ServerboundPlayChatCommand {
                    command: "help".into(),
                },
            ),
        );

        assert!(event.unwrap().as_any().is::<CommandEvent>());
    }

    #[test]
    fn unhandled_returns_none() {
        let mut player = test_player();

        let event = packet_to_event(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::CustomPayload(
                basalt_protocol::packets::play::misc::ServerboundPlayCustomPayload {
                    channel: "brand".into(),
                    data: vec![],
                },
            ),
        );

        assert!(event.is_none());
    }

    #[test]
    fn block_dig_status_0_returns_event() {
        let mut player = test_player();

        let event = packet_to_event(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::BlockDig(
                basalt_protocol::packets::play::world::ServerboundPlayBlockDig {
                    status: 0,
                    location: Position::new(10, 64, -5),
                    face: 1,
                    sequence: 42,
                },
            ),
        );

        let event = event.unwrap();
        let broken = event.as_any().downcast_ref::<BlockBrokenEvent>().unwrap();
        assert_eq!(broken.x, 10);
        assert_eq!(broken.y, 64);
        assert_eq!(broken.z, -5);
        assert_eq!(broken.sequence, 42);
    }

    #[test]
    fn block_dig_other_status_returns_none() {
        let mut player = test_player();

        let event = packet_to_event(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::BlockDig(
                basalt_protocol::packets::play::world::ServerboundPlayBlockDig {
                    status: 1,
                    location: Position::new(0, 0, 0),
                    face: 0,
                    sequence: 1,
                },
            ),
        );

        assert!(event.is_none());
    }

    #[test]
    fn block_place_with_valid_item() {
        let mut player = test_player();
        player.hotbar[0] = basalt_types::Slot::new(1, 64);
        player.held_slot = 0;

        let event = packet_to_event(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::BlockPlace(
                basalt_protocol::packets::play::world::ServerboundPlayBlockPlace {
                    hand: 0,
                    location: Position::new(5, 63, 5),
                    direction: 1,
                    cursor_x: 0.5,
                    cursor_y: 1.0,
                    cursor_z: 0.5,
                    inside_block: false,
                    world_border_hit: false,
                    sequence: 7,
                },
            ),
        );

        let event = event.unwrap();
        let placed = event.as_any().downcast_ref::<BlockPlacedEvent>().unwrap();
        assert_eq!(placed.x, 5);
        assert_eq!(placed.y, 64);
        assert_eq!(placed.z, 5);
        assert_eq!(placed.block_state, 1);
        assert_eq!(placed.sequence, 7);
    }

    #[test]
    fn block_place_empty_hand_returns_none() {
        let mut player = test_player();

        let event = packet_to_event(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::BlockPlace(
                basalt_protocol::packets::play::world::ServerboundPlayBlockPlace {
                    hand: 0,
                    location: Position::new(0, 64, 0),
                    direction: 1,
                    cursor_x: 0.5,
                    cursor_y: 1.0,
                    cursor_z: 0.5,
                    inside_block: false,
                    world_border_hit: false,
                    sequence: 1,
                },
            ),
        );

        assert!(event.is_none());
    }

    #[test]
    fn held_item_slot_updates_player() {
        let mut player = test_player();
        assert_eq!(player.held_slot, 0);

        let event = packet_to_event(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::HeldItemSlot(
                basalt_protocol::packets::play::inventory::ServerboundPlayHeldItemSlot {
                    slot_id: 4,
                },
            ),
        );

        assert_eq!(player.held_slot, 4);
        assert!(event.is_none());
    }

    #[test]
    fn set_creative_slot_updates_hotbar() {
        let mut player = test_player();

        let event = packet_to_event(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::SetCreativeSlot(
                basalt_protocol::packets::play::inventory::ServerboundPlaySetCreativeSlot {
                    slot: 36,
                    item: basalt_types::Slot::new(1, 64),
                },
            ),
        );

        assert_eq!(player.hotbar[0].item_id, Some(1));
        assert_eq!(player.hotbar[0].item_count, 64);
        assert!(event.is_none());
    }

    #[test]
    fn face_offset_all_directions() {
        assert_eq!(face_offset(0), (0, -1, 0)); // bottom
        assert_eq!(face_offset(1), (0, 1, 0)); // top
        assert_eq!(face_offset(2), (0, 0, -1)); // north
        assert_eq!(face_offset(3), (0, 0, 1)); // south
        assert_eq!(face_offset(4), (-1, 0, 0)); // west
        assert_eq!(face_offset(5), (1, 0, 0)); // east
        assert_eq!(face_offset(99), (0, 0, 0)); // invalid
    }
}
