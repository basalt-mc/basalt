//! Minimal Minecraft server with empty void world.
//!
//! - Status flow: responds with server info + ping
//! - Login flow: offline mode (no Mojang auth), transitions to Configuration
//! - Configuration: sends minimum registry data, transitions to Play
//! - Play: sends login, spawn position, chunk, player position, keep-alive
//!
//! The player spawns in creative mode in a void world at (0, 100, 0).
//!
//! Usage: `cargo run --package basalt-net --example server`
//! Then open Minecraft 1.21.x (offline mode) and connect to `localhost:25565`.

use basalt_net::connection::{Connection, Handshake, HandshakeResult};
use basalt_protocol::chunk::build_empty_chunk;
use basalt_protocol::packets::configuration::ClientboundConfigurationRegistryData;
use basalt_protocol::packets::login::{ClientboundLoginSuccess, ServerboundLoginPacket};
use basalt_protocol::packets::play::misc::ClientboundPlayKeepAlive;
use basalt_protocol::packets::play::player::{
    ClientboundPlayGameStateChange, ClientboundPlayLogin, ClientboundPlayPosition,
};
use basalt_protocol::packets::play::world::{
    ClientboundPlayMapChunk, ClientboundPlaySpawnPosition,
};
use basalt_protocol::packets::status::{
    ClientboundStatusPing, ClientboundStatusServerInfo, ServerboundStatusPacket,
};
use basalt_protocol::registry_data::build_default_registries;
use basalt_types::{Encode, Position, VarInt};
use tokio::net::TcpListener;

const SERVER_STATUS: &str = r#"{
    "version": {
        "name": "Basalt 1.21.4",
        "protocol": 769
    },
    "players": {
        "max": 20,
        "online": 0,
        "sample": []
    },
    "description": {
        "text": "A Basalt Server — Empty World"
    },
    "enforcesSecureChat": false
}"#;

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("0.0.0.0:25565").await.unwrap();
    println!("Basalt server listening on 0.0.0.0:25565");

    loop {
        let (stream, addr) = listener.accept().await.unwrap();
        println!("[{addr}] Connection accepted");

        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, addr).await {
                println!("[{addr}] Error: {e}");
            }
            println!("[{addr}] Connection closed");
        });
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    addr: std::net::SocketAddr,
) -> basalt_net::Result<()> {
    let conn = Connection::<Handshake>::accept(stream);

    match conn.read_handshake().await? {
        HandshakeResult::Status(conn, handshake) => {
            println!(
                "[{addr}] Status request (protocol {})",
                handshake.protocol_version
            );
            handle_status(conn, addr).await
        }
        HandshakeResult::Login(conn, handshake) => {
            println!(
                "[{addr}] Login request (protocol {})",
                handshake.protocol_version
            );
            handle_login(conn, addr).await
        }
    }
}

async fn handle_status(
    mut conn: Connection<basalt_net::connection::Status>,
    addr: std::net::SocketAddr,
) -> basalt_net::Result<()> {
    let packet = conn.read_packet().await?;
    if let ServerboundStatusPacket::PingStart(_) = packet {
        println!("[{addr}] <- StatusRequest");
    }

    let response = ClientboundStatusServerInfo {
        response: SERVER_STATUS.into(),
    };
    conn.write_status_response(&response).await?;
    println!("[{addr}] -> StatusResponse");

    let packet = conn.read_packet().await?;
    if let ServerboundStatusPacket::Ping(ping) = packet {
        println!("[{addr}] <- Ping (time={})", ping.time);
        let pong = ClientboundStatusPing { time: ping.time };
        conn.write_ping_response(&pong).await?;
        println!("[{addr}] -> Pong");
    }

    Ok(())
}

async fn handle_login(
    mut conn: Connection<basalt_net::connection::Login>,
    addr: std::net::SocketAddr,
) -> basalt_net::Result<()> {
    // Read LoginStart
    let (username, player_uuid) = match conn.read_packet().await? {
        ServerboundLoginPacket::LoginStart(login) => {
            println!(
                "[{addr}] <- LoginStart (username={}, uuid={})",
                login.username, login.player_uuid
            );
            (login.username, login.player_uuid)
        }
        _ => {
            println!("[{addr}] <- Unexpected packet, expected LoginStart");
            return Ok(());
        }
    };

    // Send LoginSuccess → wait for LoginAcknowledged → transition to Configuration
    let success = ClientboundLoginSuccess {
        uuid: player_uuid,
        username: username.clone(),
        properties: vec![],
    };
    println!("[{addr}] -> LoginSuccess");
    let conn = conn.send_login_success(&success).await?;
    println!("[{addr}] <- LoginAcknowledged → Configuration");

    // Configuration: send registries
    handle_configuration(conn, addr, &username).await
}

async fn handle_configuration(
    mut conn: Connection<basalt_net::connection::Configuration>,
    addr: std::net::SocketAddr,
    username: &str,
) -> basalt_net::Result<()> {
    // Send all required registries
    let registries = build_default_registries();
    for reg in &registries {
        conn.write_packet_typed(ClientboundConfigurationRegistryData::PACKET_ID, reg)
            .await?;
        println!("[{addr}] -> RegistryData ({})", reg.id);
    }

    // FinishConfiguration → wait for client ack → transition to Play
    let conn = conn.finish_configuration().await?;
    println!("[{addr}] <- FinishConfiguration → Play");

    // Play: send login, chunk, position
    handle_play(conn, addr, username).await
}

async fn handle_play(
    mut conn: Connection<basalt_net::connection::Play>,
    addr: std::net::SocketAddr,
    username: &str,
) -> basalt_net::Result<()> {
    // Encode Login (Play) manually because world_state is an inline
    // container that the codegen mapped as Vec<u8> with a length prefix,
    // but the protocol expects the fields inline without a length prefix.
    let login_payload = build_login_play_payload();
    conn.write_packet_typed(ClientboundPlayLogin::PACKET_ID, &RawPayload(login_payload))
        .await?;
    println!("[{addr}] -> Login (Play)");

    // Send default spawn position
    let spawn = ClientboundPlaySpawnPosition {
        location: Position::new(0, 100, 0),
        angle: 0.0,
    };
    conn.write_packet_typed(ClientboundPlaySpawnPosition::PACKET_ID, &spawn)
        .await?;
    println!("[{addr}] -> SpawnPosition");

    // Send GameEvent: "start waiting for level chunks" (reason=13)
    let game_event = ClientboundPlayGameStateChange {
        reason: 13,
        game_mode: 0.0,
    };
    conn.write_packet_typed(ClientboundPlayGameStateChange::PACKET_ID, &game_event)
        .await?;
    println!("[{addr}] -> GameEvent (start waiting for chunks)");

    // Send empty chunk at spawn
    let chunk = build_empty_chunk(0, 0);
    conn.write_packet_typed(ClientboundPlayMapChunk::PACKET_ID, &chunk)
        .await?;
    println!("[{addr}] -> ChunkData (0, 0)");

    // Send player position
    let position = ClientboundPlayPosition {
        teleport_id: 1,
        x: 0.0,
        y: 100.0,
        z: 0.0,
        dx: 0.0,
        dy: 0.0,
        dz: 0.0,
        yaw: 0.0,
        pitch: 0.0,
        flags: build_position_flags(0),
    };
    conn.write_packet_typed(ClientboundPlayPosition::PACKET_ID, &position)
        .await?;
    println!("[{addr}] -> PlayerPosition (0, 100, 0)");

    println!("[{addr}] {username} joined the void world! Starting keep-alive loop.");

    // Keep-alive loop
    let mut keep_alive_id: i64 = 0;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(15)) => {
                keep_alive_id += 1;
                let ka = ClientboundPlayKeepAlive { keep_alive_id };
                conn.write_packet_typed(ClientboundPlayKeepAlive::PACKET_ID, &ka).await?;
            }
            result = conn.read_packet() => {
                match result {
                    Ok(_packet) => {
                        // Ignore all packets for now — just keep the connection alive
                    }
                    Err(_) => {
                        println!("[{addr}] {username} disconnected");
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

/// A wrapper that encodes raw bytes directly without any framing.
///
/// Used when we need to encode a packet manually because the codegen
/// produced a struct with incorrect field types (e.g., Vec<u8> with
/// length prefix where inline bytes are expected).
struct RawPayload(Vec<u8>);

impl Encode for RawPayload {
    fn encode(&self, buf: &mut Vec<u8>) -> basalt_types::Result<()> {
        buf.extend_from_slice(&self.0);
        Ok(())
    }
}

impl basalt_types::EncodedSize for RawPayload {
    fn encoded_size(&self) -> usize {
        self.0.len()
    }
}

/// Builds the complete Login (Play) packet payload manually.
///
/// The codegen maps `world_state` (SpawnInfo) as `Vec<u8>` with a length
/// prefix, but the protocol expects the SpawnInfo fields inline. We encode
/// the entire packet by hand to get the correct wire format.
fn build_login_play_payload() -> Vec<u8> {
    let mut buf = Vec::new();

    // entity_id: i32
    1i32.encode(&mut buf).unwrap();
    // is_hardcore: bool
    false.encode(&mut buf).unwrap();
    // world_names: array of String
    VarInt(1).encode(&mut buf).unwrap();
    "minecraft:overworld".to_string().encode(&mut buf).unwrap();
    // max_players: VarInt
    VarInt(20).encode(&mut buf).unwrap();
    // view_distance: VarInt
    VarInt(10).encode(&mut buf).unwrap();
    // simulation_distance: VarInt
    VarInt(10).encode(&mut buf).unwrap();
    // reduced_debug_info: bool
    false.encode(&mut buf).unwrap();
    // enable_respawn_screen: bool
    true.encode(&mut buf).unwrap();
    // do_limited_crafting: bool
    false.encode(&mut buf).unwrap();

    // --- SpawnInfo (world_state) inline, NOT length-prefixed ---
    // dimension: VarInt (index into dimension_type registry)
    VarInt(0).encode(&mut buf).unwrap();
    // name: String (dimension name)
    "minecraft:overworld".to_string().encode(&mut buf).unwrap();
    // hashed_seed: i64
    0i64.encode(&mut buf).unwrap();
    // game_mode: i8 (1 = creative)
    1i8.encode(&mut buf).unwrap();
    // previous_game_mode: u8 (255 = none)
    255u8.encode(&mut buf).unwrap();
    // is_debug: bool
    false.encode(&mut buf).unwrap();
    // is_flat: bool
    true.encode(&mut buf).unwrap();
    // death: Option (false = no death info)
    false.encode(&mut buf).unwrap();
    // portal_cooldown: VarInt
    VarInt(0).encode(&mut buf).unwrap();
    // sea_level: VarInt
    VarInt(63).encode(&mut buf).unwrap();

    // enforces_secure_chat: bool
    false.encode(&mut buf).unwrap();

    buf
}

/// Builds the position flags bitfield as a VarInt-prefixed bitset.
fn build_position_flags(flags: i32) -> Vec<u8> {
    let mut buf = Vec::new();
    VarInt(flags).encode(&mut buf).unwrap();
    buf
}
