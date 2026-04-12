//! Play state loop with packet dispatch.
//!
//! Handles the main gameplay loop: sends initial world data (login,
//! chunks, position), then enters a read loop that dispatches incoming
//! packets to the appropriate handlers while sending periodic keep-alive
//! probes.

use std::net::SocketAddr;
use std::time::Instant;

use basalt_net::connection::{Connection, Play};
use basalt_protocol::chunk::build_empty_chunk;
use basalt_protocol::packets::play::ServerboundPlayPacket;
use basalt_protocol::packets::play::misc::ClientboundPlayKeepAlive;
use basalt_protocol::packets::play::player::{
    ClientboundPlayGameStateChange, ClientboundPlayLogin, ClientboundPlayLoginSpawninfo,
    ClientboundPlayPosition,
};
use basalt_protocol::packets::play::world::{
    ClientboundPlayMapChunk, ClientboundPlaySpawnPosition,
};
use basalt_types::Position;

use crate::player::PlayerState;

/// Sends the initial world data to the client and enters the play loop.
///
/// This is the entry point for the Play state. It sends the Login
/// packet, spawn position, game event, chunk data, and player position,
/// then enters the main read/write loop.
pub(crate) async fn run_play_loop(
    mut conn: Connection<Play>,
    addr: SocketAddr,
    player: &mut PlayerState,
) -> basalt_net::Result<()> {
    send_initial_world(&mut conn, addr, player).await?;

    println!(
        "[{addr}] {} joined the void world! Starting play loop.",
        player.username
    );

    play_loop(&mut conn, addr, player).await
}

/// Sends the initial world data that the client needs to enter the game.
///
/// Order matters: Login → SpawnPosition → GameEvent (start waiting for
/// chunks) → ChunkData → PlayerPosition. The client won't render the
/// world until it receives all of these in the correct order.
async fn send_initial_world(
    conn: &mut Connection<Play>,
    addr: SocketAddr,
    player: &PlayerState,
) -> basalt_net::Result<()> {
    // Login (Play) — tells the client about the world
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
            gamemode: 1,            // creative
            previous_gamemode: 255, // none
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

    // Spawn position
    let spawn = ClientboundPlaySpawnPosition {
        location: Position::new(0, 100, 0),
        angle: 0.0,
    };
    conn.write_packet_typed(ClientboundPlaySpawnPosition::PACKET_ID, &spawn)
        .await?;
    println!("[{addr}] -> SpawnPosition");

    // GameEvent: "start waiting for level chunks" (reason=13)
    let game_event = ClientboundPlayGameStateChange {
        reason: 13,
        game_mode: 0.0,
    };
    conn.write_packet_typed(ClientboundPlayGameStateChange::PACKET_ID, &game_event)
        .await?;
    println!("[{addr}] -> GameEvent (start waiting for chunks)");

    // Empty chunk at spawn
    let chunk = build_empty_chunk(0, 0);
    conn.write_packet_typed(ClientboundPlayMapChunk::PACKET_ID, &chunk)
        .await?;
    println!("[{addr}] -> ChunkData (0, 0)");

    // Player position — flags=0 means all coordinates are absolute
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

/// Main play loop: reads client packets and sends periodic keep-alive.
///
/// Runs until the client disconnects or an error occurs. Each incoming
/// packet is dispatched to the appropriate handler based on its type.
async fn play_loop(
    conn: &mut Connection<Play>,
    addr: SocketAddr,
    player: &mut PlayerState,
) -> basalt_net::Result<()> {
    // Use interval instead of sleep — sleep is cancelled and reset
    // each time a packet arrives, so keep-alive would never fire
    // because the client sends movement packets every tick (~50ms).
    let mut keep_alive = tokio::time::interval(std::time::Duration::from_secs(15));
    keep_alive.tick().await; // skip the immediate first tick

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
                    Ok(packet) => dispatch_packet(addr, player, packet),
                    Err(e) => {
                        println!("[{addr}] {} disconnected: {e}", player.username);
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Dispatches a single serverbound Play packet to the appropriate handler.
///
/// This is a pure function that updates `PlayerState` without any IO.
/// Packets that require sending a response (chat, commands) will return
/// an action in a future iteration; for now they are logged.
pub(crate) fn dispatch_packet(
    addr: SocketAddr,
    player: &mut PlayerState,
    packet: ServerboundPlayPacket,
) {
    match packet {
        // -- Keep-alive --
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
        }

        // -- Teleport confirm --
        ServerboundPlayPacket::TeleportConfirm(tc) => {
            println!(
                "[{addr}] {} confirmed teleport (id={})",
                player.username, tc.teleport_id
            );
            player.teleport_confirmed = true;
        }

        // -- Player loaded --
        ServerboundPlayPacket::PlayerLoaded(_) => {
            println!("[{addr}] {} finished loading", player.username);
            player.loaded = true;
        }

        // -- Movement --
        ServerboundPlayPacket::Position(p) => {
            player.update_position(p.x, p.y, p.z);
            player.update_on_ground(p.flags);
        }
        ServerboundPlayPacket::PositionLook(p) => {
            player.update_position(p.x, p.y, p.z);
            player.update_look(p.yaw, p.pitch);
            player.update_on_ground(p.flags);
        }
        ServerboundPlayPacket::Look(p) => {
            player.update_look(p.yaw, p.pitch);
            player.update_on_ground(p.flags);
        }
        ServerboundPlayPacket::Flying(p) => {
            player.update_on_ground(p.flags);
        }

        // -- Chat (logged for now, handled in #52) --
        ServerboundPlayPacket::ChatMessage(msg) => {
            println!("[{addr}] <{}> {}", player.username, msg.message);
        }
        ServerboundPlayPacket::ChatCommand(cmd) => {
            println!(
                "[{addr}] {} issued command: /{}",
                player.username, cmd.command
            );
        }

        // -- Client settings / plugin channels (acknowledged silently) --
        ServerboundPlayPacket::CustomPayload(_)
        | ServerboundPlayPacket::PlayerInput(_)
        | ServerboundPlayPacket::TickEnd(_)
        | ServerboundPlayPacket::ChunkBatchReceived(_)
        | ServerboundPlayPacket::Pong(_)
        | ServerboundPlayPacket::MessageAcknowledgement(_)
        | ServerboundPlayPacket::ConfigurationAcknowledged(_) => {
            // Expected protocol traffic — handle silently
        }

        // -- Everything else --
        other => {
            println!(
                "[{addr}] {} sent unhandled packet: {:?}",
                player.username,
                std::mem::discriminant(&other)
            );
        }
    }
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
        PlayerState::new("Steve".into(), Uuid::default(), 1)
    }

    fn test_addr() -> SocketAddr {
        "127.0.0.1:12345".parse().unwrap()
    }

    #[test]
    fn dispatch_teleport_confirm() {
        let mut player = test_player();
        assert!(!player.teleport_confirmed);

        dispatch_packet(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::TeleportConfirm(ServerboundPlayTeleportConfirm {
                teleport_id: 1,
            }),
        );

        assert!(player.teleport_confirmed);
    }

    #[test]
    fn dispatch_player_loaded() {
        let mut player = test_player();
        assert!(!player.loaded);

        dispatch_packet(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::PlayerLoaded(
                basalt_protocol::packets::play::player::ServerboundPlayPlayerLoaded,
            ),
        );

        assert!(player.loaded);
    }

    #[test]
    fn dispatch_keep_alive_matching() {
        let mut player = test_player();
        player.last_keep_alive_id = 42;

        dispatch_packet(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::KeepAlive(ServerboundPlayKeepAlive { keep_alive_id: 42 }),
        );

        // No state change — just logged, no panic
    }

    #[test]
    fn dispatch_keep_alive_mismatch() {
        let mut player = test_player();
        player.last_keep_alive_id = 42;

        dispatch_packet(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::KeepAlive(ServerboundPlayKeepAlive { keep_alive_id: 999 }),
        );

        // Mismatched — logged, no panic
    }

    #[test]
    fn dispatch_position() {
        let mut player = test_player();
        assert_eq!(player.x, 0.0);

        dispatch_packet(
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
    }

    #[test]
    fn dispatch_position_look() {
        let mut player = test_player();

        dispatch_packet(
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
        assert_eq!(player.pitch, -45.0);
        assert!(!player.on_ground);
    }

    #[test]
    fn dispatch_look() {
        let mut player = test_player();

        dispatch_packet(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::Look(ServerboundPlayLook {
                yaw: 180.0,
                pitch: 0.0,
                flags: 0x01,
            }),
        );

        assert_eq!(player.yaw, 180.0);
        assert_eq!(player.pitch, 0.0);
        assert!(player.on_ground);
    }

    #[test]
    fn dispatch_flying() {
        let mut player = test_player();
        assert!(!player.on_ground);

        dispatch_packet(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::Flying(ServerboundPlayFlying { flags: 0x01 }),
        );

        assert!(player.on_ground);
    }

    #[test]
    fn dispatch_unhandled_does_not_panic() {
        let mut player = test_player();

        // Should log but not panic
        dispatch_packet(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::ArmAnimation(
                basalt_protocol::packets::play::entity::ServerboundPlayArmAnimation { hand: 0 },
            ),
        );
    }

    #[test]
    fn dispatch_silent_packets_do_not_panic() {
        let mut player = test_player();

        dispatch_packet(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::CustomPayload(
                basalt_protocol::packets::play::misc::ServerboundPlayCustomPayload {
                    channel: "minecraft:brand".into(),
                    data: vec![],
                },
            ),
        );

        dispatch_packet(
            test_addr(),
            &mut player,
            ServerboundPlayPacket::TickEnd(
                basalt_protocol::packets::play::misc::ServerboundPlayTickEnd,
            ),
        );
    }
}
