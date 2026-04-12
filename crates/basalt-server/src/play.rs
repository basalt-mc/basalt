//! Play state loop with packet dispatch and multi-player broadcast.
//!
//! Handles the main gameplay loop: sends initial world data (login,
//! chunks, position), then enters a read loop that dispatches incoming
//! packets, sends periodic keep-alive probes, and processes broadcast
//! messages from other players (chat, join/leave, movement).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use crate::chunk::build_empty_chunk;
use basalt_net::connection::{Connection, Play};
use basalt_protocol::packets::play::ServerboundPlayPacket;
use basalt_protocol::packets::play::chat::ClientboundPlaySystemChat;
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
    ClientboundPlayChunkBatchFinished, ClientboundPlayChunkBatchStart, ClientboundPlayMapChunk,
    ClientboundPlaySpawnPosition,
};
use basalt_types::{Encode, Position, VarInt, Vec3i16};
use tokio::sync::mpsc;

use crate::helpers::{RawPayload, angle_to_byte};
use crate::player::PlayerState;
use crate::state::{BroadcastMessage, PlayerSnapshot, ServerState};

/// Sends the initial world data to the client and enters the play loop.
pub(crate) async fn run_play_loop(
    mut conn: Connection<Play>,
    addr: SocketAddr,
    player: &mut PlayerState,
    state: &Arc<ServerState>,
    rx: mpsc::Receiver<BroadcastMessage>,
    existing_players: &[PlayerSnapshot],
) -> basalt_net::Result<()> {
    send_initial_world(&mut conn, addr, player).await?;

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

    println!(
        "[{addr}] {} joined the void world! Starting play loop.",
        player.username
    );

    play_loop(&mut conn, addr, player, state, rx).await
}

/// Sends the initial world data that the client needs to enter the game.
async fn send_initial_world(
    conn: &mut Connection<Play>,
    addr: SocketAddr,
    player: &PlayerState,
) -> basalt_net::Result<()> {
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
    println!("[{addr}] -> Login (Play)");

    let spawn = ClientboundPlaySpawnPosition {
        location: Position::new(0, 100, 0),
        angle: 0.0,
    };
    conn.write_packet_typed(ClientboundPlaySpawnPosition::PACKET_ID, &spawn)
        .await?;
    println!("[{addr}] -> SpawnPosition");

    let game_event = ClientboundPlayGameStateChange {
        reason: 13,
        game_mode: 0.0,
    };
    conn.write_packet_typed(ClientboundPlayGameStateChange::PACKET_ID, &game_event)
        .await?;
    println!("[{addr}] -> GameEvent (start waiting for chunks)");

    // Send a grid of empty chunks around spawn so the player can
    // walk around without falling into the void. The view_distance
    // in the Login packet is 10, so we send a 7x7 grid (enough to
    // fill the immediate view without sending hundreds of chunks).
    let radius = 3;
    let batch_start = ClientboundPlayChunkBatchStart;
    conn.write_packet_typed(ClientboundPlayChunkBatchStart::PACKET_ID, &batch_start)
        .await?;

    let mut chunk_count = 0;
    for cx in -radius..=radius {
        for cz in -radius..=radius {
            let chunk = build_empty_chunk(cx, cz);
            conn.write_packet_typed(ClientboundPlayMapChunk::PACKET_ID, &chunk)
                .await?;
            chunk_count += 1;
        }
    }

    let batch_finish = ClientboundPlayChunkBatchFinished {
        batch_size: chunk_count,
    };
    conn.write_packet_typed(ClientboundPlayChunkBatchFinished::PACKET_ID, &batch_finish)
        .await?;
    println!("[{addr}] -> ChunkData ({chunk_count} chunks, radius {radius})");

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
    println!(
        "[{addr}] -> PlayerPosition ({}, {}, {})",
        player.x, player.y, player.z
    );

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
    mut rx: mpsc::Receiver<BroadcastMessage>,
) -> basalt_net::Result<()> {
    let mut keep_alive = tokio::time::interval(std::time::Duration::from_secs(15));
    keep_alive.tick().await;

    loop {
        tokio::select! {
            _ = keep_alive.tick() => {
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
                        let action = dispatch_packet(addr, player, packet);
                        execute_action(conn, player, state, action).await?;
                    }
                    Err(basalt_net::Error::Protocol(
                        basalt_protocol::Error::UnknownPacket { id, .. }
                    )) => {
                        // Common packets (settings, plugin channels) are
                        // skipped by the codegen and produce UnknownPacket.
                        // Ignore them silently.
                        println!("[{addr}] {} sent unknown packet 0x{id:02x}, ignoring", player.username);
                    }
                    Err(e) => {
                        println!("[{addr}] {} disconnected: {e}", player.username);
                        break;
                    }
                }
            }
            Some(msg) = rx.recv() => {
                handle_broadcast(conn, player, msg).await?;
            }
        }
    }

    Ok(())
}

/// Result of dispatching a packet synchronously.
pub(crate) enum PacketAction {
    /// Packet was fully handled (state updated, logged).
    Handled,
    /// A chat message that needs to be broadcast.
    Chat { username: String, message: String },
    /// A command that needs to be executed.
    Command { command: String },
    /// The player moved — broadcast to other players.
    Moved,
}

/// Dispatches a single serverbound Play packet synchronously.
pub(crate) fn dispatch_packet(
    addr: SocketAddr,
    player: &mut PlayerState,
    packet: ServerboundPlayPacket,
) -> PacketAction {
    match packet {
        ServerboundPlayPacket::KeepAlive(ka) => {
            if ka.keep_alive_id == player.last_keep_alive_id {
                let rtt = player.last_keep_alive_sent.elapsed();
                println!(
                    "[{addr}] {} keep-alive OK (RTT: {}ms)",
                    player.username,
                    rtt.as_millis()
                );
            } else {
                println!(
                    "[{addr}] {} keep-alive mismatch: expected {}, got {}",
                    player.username, player.last_keep_alive_id, ka.keep_alive_id
                );
            }
            PacketAction::Handled
        }
        ServerboundPlayPacket::TeleportConfirm(tc) => {
            println!(
                "[{addr}] {} confirmed teleport (id={})",
                player.username, tc.teleport_id
            );
            player.teleport_confirmed = true;
            PacketAction::Handled
        }
        ServerboundPlayPacket::PlayerLoaded(_) => {
            println!("[{addr}] {} finished loading", player.username);
            player.loaded = true;
            PacketAction::Handled
        }
        ServerboundPlayPacket::Position(p) => {
            player.update_position(p.x, p.y, p.z);
            player.update_on_ground(p.flags);
            PacketAction::Moved
        }
        ServerboundPlayPacket::PositionLook(p) => {
            player.update_position(p.x, p.y, p.z);
            player.update_look(p.yaw, p.pitch);
            player.update_on_ground(p.flags);
            PacketAction::Moved
        }
        ServerboundPlayPacket::Look(p) => {
            player.update_look(p.yaw, p.pitch);
            player.update_on_ground(p.flags);
            PacketAction::Moved
        }
        ServerboundPlayPacket::Flying(p) => {
            player.update_on_ground(p.flags);
            PacketAction::Handled
        }
        ServerboundPlayPacket::ChatMessage(msg) => {
            println!("[{addr}] <{}> {}", player.username, msg.message);
            PacketAction::Chat {
                username: player.username.clone(),
                message: msg.message,
            }
        }
        ServerboundPlayPacket::ChatCommand(cmd) => {
            println!(
                "[{addr}] {} issued command: /{}",
                player.username, cmd.command
            );
            PacketAction::Command {
                command: cmd.command,
            }
        }
        ServerboundPlayPacket::CustomPayload(_)
        | ServerboundPlayPacket::PlayerInput(_)
        | ServerboundPlayPacket::TickEnd(_)
        | ServerboundPlayPacket::ChunkBatchReceived(_)
        | ServerboundPlayPacket::Pong(_)
        | ServerboundPlayPacket::MessageAcknowledgement(_)
        | ServerboundPlayPacket::ConfigurationAcknowledged(_) => PacketAction::Handled,
        other => {
            println!(
                "[{addr}] {} sent unhandled packet: {:?}",
                player.username,
                std::mem::discriminant(&other)
            );
            PacketAction::Handled
        }
    }
}

/// Executes the async part of a packet action.
async fn execute_action(
    conn: &mut Connection<Play>,
    player: &mut PlayerState,
    state: &Arc<ServerState>,
    action: PacketAction,
) -> basalt_net::Result<()> {
    match action {
        PacketAction::Handled => {}
        PacketAction::Chat { username, message } => {
            let content = crate::chat::build_chat_component(&username, &message).to_nbt();
            state.broadcast(BroadcastMessage::Chat { content }).await;
        }
        PacketAction::Command { command } => {
            crate::chat::handle_command(conn, player, &command).await?;
        }
        PacketAction::Moved => {
            state
                .broadcast_except(
                    BroadcastMessage::EntityMoved {
                        entity_id: player.entity_id,
                        x: player.x,
                        y: player.y,
                        z: player.z,
                        yaw: player.yaw,
                        pitch: player.pitch,
                        on_ground: player.on_ground,
                    },
                    &player.uuid,
                )
                .await;
        }
    }
    Ok(())
}

/// Handles an incoming broadcast message from another player.
async fn handle_broadcast(
    conn: &mut Connection<Play>,
    player: &PlayerState,
    msg: BroadcastMessage,
) -> basalt_net::Result<()> {
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
) -> basalt_net::Result<()> {
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
        .await
}

/// Sends a SpawnEntity packet for a player entity.
///
/// Player entities use type ID 128 in Minecraft 1.21.4.
async fn send_spawn_entity(
    conn: &mut Connection<Play>,
    info: &PlayerSnapshot,
) -> basalt_net::Result<()> {
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
        .await
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
    fn dispatch_teleport_confirm() {
        let mut player = test_player();
        assert!(!player.teleport_confirmed);

        let action = dispatch_packet(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::TeleportConfirm(ServerboundPlayTeleportConfirm {
                teleport_id: 1,
            }),
        );

        assert!(player.teleport_confirmed);
        assert!(matches!(action, PacketAction::Handled));
    }

    #[test]
    fn dispatch_player_loaded() {
        let mut player = test_player();
        assert!(!player.loaded);

        let action = dispatch_packet(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::PlayerLoaded(
                basalt_protocol::packets::play::player::ServerboundPlayPlayerLoaded,
            ),
        );

        assert!(player.loaded);
        assert!(matches!(action, PacketAction::Handled));
    }

    #[test]
    fn dispatch_keep_alive_matching() {
        let mut player = test_player();
        player.last_keep_alive_id = 42;

        let action = dispatch_packet(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::KeepAlive(ServerboundPlayKeepAlive { keep_alive_id: 42 }),
        );

        assert!(matches!(action, PacketAction::Handled));
    }

    #[test]
    fn dispatch_position_returns_moved() {
        let mut player = test_player();

        let action = dispatch_packet(
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
        assert!(matches!(action, PacketAction::Moved));
    }

    #[test]
    fn dispatch_position_look_returns_moved() {
        let mut player = test_player();

        let action = dispatch_packet(
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
        assert!(matches!(action, PacketAction::Moved));
    }

    #[test]
    fn dispatch_look_returns_moved() {
        let mut player = test_player();

        let action = dispatch_packet(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::Look(ServerboundPlayLook {
                yaw: 180.0,
                pitch: 0.0,
                flags: 0x01,
            }),
        );

        assert_eq!(player.yaw, 180.0);
        assert!(matches!(action, PacketAction::Moved));
    }

    #[test]
    fn dispatch_flying_returns_handled() {
        let mut player = test_player();

        let action = dispatch_packet(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::Flying(ServerboundPlayFlying { flags: 0x01 }),
        );

        assert!(player.on_ground);
        assert!(matches!(action, PacketAction::Handled));
    }

    #[test]
    fn dispatch_chat_returns_action() {
        let mut player = test_player();

        let action = dispatch_packet(
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

        assert!(matches!(action, PacketAction::Chat { .. }));
    }

    #[test]
    fn dispatch_command_returns_action() {
        let mut player = test_player();

        let action = dispatch_packet(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::ChatCommand(
                basalt_protocol::packets::play::chat::ServerboundPlayChatCommand {
                    command: "help".into(),
                },
            ),
        );

        assert!(matches!(action, PacketAction::Command { .. }));
    }

    #[test]
    fn dispatch_unhandled_does_not_panic() {
        let mut player = test_player();

        let action = dispatch_packet(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::ArmAnimation(
                basalt_protocol::packets::play::entity::ServerboundPlayArmAnimation { hand: 0 },
            ),
        );

        assert!(matches!(action, PacketAction::Handled));
    }
}
