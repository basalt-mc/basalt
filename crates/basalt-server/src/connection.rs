//! Per-player connection handler.
//!
//! Manages the full lifecycle of a client connection: Handshake →
//! Status or Login → Configuration → Play. Each state transition
//! consumes the connection and produces a new one in the target state.

use std::net::SocketAddr;

use basalt_net::connection::{Connection, Handshake, HandshakeResult};
use basalt_protocol::packets::configuration::ClientboundConfigurationRegistryData;
use basalt_protocol::packets::login::{ClientboundLoginSuccess, ServerboundLoginPacket};
use basalt_protocol::packets::status::{
    ClientboundStatusPing, ClientboundStatusServerInfo, ServerboundStatusPacket,
};
use basalt_protocol::registry_data::build_default_registries;

use crate::play::run_play_loop;
use crate::player::PlayerState;

/// JSON response for the server list ping.
///
/// Tells the Minecraft client the server name, protocol version,
/// player count, and description. Displayed in the server browser.
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
        "text": "A Basalt Server"
    },
    "enforcesSecureChat": false
}"#;

/// Handles a new TCP connection from start to finish.
///
/// Reads the handshake to determine whether this is a status ping or
/// a login attempt, then delegates to the appropriate handler. The
/// connection is fully consumed when this function returns.
pub(crate) async fn handle_connection(
    stream: tokio::net::TcpStream,
    addr: SocketAddr,
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

/// Handles the Status state: responds with server info and ping.
///
/// The Minecraft client sends a StatusRequest followed by a Ping.
/// We respond with the server list JSON and echo the ping timestamp.
async fn handle_status(
    mut conn: Connection<basalt_net::connection::Status>,
    addr: SocketAddr,
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

/// Handles the Login state: reads LoginStart, sends LoginSuccess,
/// transitions to Configuration, sends registries, then transitions
/// to Play.
async fn handle_login(
    mut conn: Connection<basalt_net::connection::Login>,
    addr: SocketAddr,
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

    // Send LoginSuccess → wait for LoginAcknowledged → Configuration
    let success = ClientboundLoginSuccess {
        uuid: player_uuid,
        username: username.clone(),
        properties: vec![],
    };
    println!("[{addr}] -> LoginSuccess");
    let conn = conn.send_login_success(&success).await?;
    println!("[{addr}] <- LoginAcknowledged → Configuration");

    // Configuration: send registries then transition to Play
    handle_configuration(conn, addr, &username, player_uuid).await
}

/// Handles the Configuration state: sends all required registries,
/// then sends FinishConfiguration and transitions to Play.
async fn handle_configuration(
    mut conn: Connection<basalt_net::connection::Configuration>,
    addr: SocketAddr,
    username: &str,
    player_uuid: basalt_types::Uuid,
) -> basalt_net::Result<()> {
    let registries = build_default_registries();
    for reg in &registries {
        conn.write_packet_typed(ClientboundConfigurationRegistryData::PACKET_ID, reg)
            .await?;
        println!("[{addr}] -> RegistryData ({})", reg.id);
    }

    let conn = conn.finish_configuration().await?;
    println!("[{addr}] <- FinishConfiguration → Play");

    // Create player state and enter the play loop
    let mut player = PlayerState::new(username.to_string(), player_uuid, 1);
    run_play_loop(conn, addr, &mut player).await
}
