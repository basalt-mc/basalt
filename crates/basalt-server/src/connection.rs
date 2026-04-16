//! Per-player connection handler.
//!
//! Manages the full lifecycle of a client connection: Handshake →
//! Status or Login → Configuration → Play. After reaching Play state,
//! creates per-player channels, notifies both loops, and starts the
//! net task for packet fan-out and output relay.

use std::net::SocketAddr;
use std::sync::Arc;

use basalt_net::connection::{Connection, Handshake, HandshakeResult};
use basalt_protocol::packets::configuration::ClientboundConfigurationRegistryData;
use basalt_protocol::packets::login::{ClientboundLoginSuccess, ServerboundLoginPacket};
use basalt_protocol::packets::status::{
    ClientboundStatusPing, ClientboundStatusServerInfo, ServerboundStatusPacket,
};
use basalt_protocol::registry_data::build_default_registries;
use tokio::sync::mpsc;

use crate::channels::player_output_channel;
use crate::messages::{GameInput, NetworkInput};
use crate::state::ServerState;

/// JSON response for the server list ping.
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
/// a login attempt, then delegates to the appropriate handler. Login
/// connections proceed through Configuration and into a net task wired
/// to the network and game loops.
pub(crate) async fn handle_connection(
    stream: tokio::net::TcpStream,
    addr: SocketAddr,
    state: Arc<ServerState>,
    network_tx: mpsc::UnboundedSender<NetworkInput>,
    game_tx: mpsc::UnboundedSender<GameInput>,
) -> crate::error::Result<()> {
    let conn = Connection::<Handshake>::accept(stream);

    match conn.read_handshake().await? {
        HandshakeResult::Status(conn, handshake) => {
            log::debug!(target: "basalt::connection", "[{addr}] Status request (protocol {})", handshake.protocol_version);
            handle_status(conn, addr).await
        }
        HandshakeResult::Login(conn, handshake) => {
            log::info!(target: "basalt::connection", "[{addr}] Login (protocol {})", handshake.protocol_version);
            handle_login(conn, addr, state, network_tx, game_tx).await
        }
    }
}

/// Handles the Status state: responds with server info and ping.
async fn handle_status(
    mut conn: Connection<basalt_net::connection::Status>,
    addr: SocketAddr,
) -> crate::error::Result<()> {
    let packet = conn.read_packet().await?;
    if let ServerboundStatusPacket::PingStart(_) = packet {
        log::debug!(target: "basalt::connection", "[{addr}] <- StatusRequest");
    }

    let response = ClientboundStatusServerInfo {
        response: SERVER_STATUS.into(),
    };
    conn.write_status_response(&response).await?;
    log::debug!(target: "basalt::connection", "[{addr}] -> StatusResponse");

    let packet = conn.read_packet().await?;
    if let ServerboundStatusPacket::Ping(ping) = packet {
        log::debug!(target: "basalt::connection", "[{addr}] <- Ping (time={})", ping.time);
        let pong = ClientboundStatusPing { time: ping.time };
        conn.write_ping_response(&pong).await?;
        log::debug!(target: "basalt::connection", "[{addr}] -> Pong");
    }

    Ok(())
}

/// Handles Login → Configuration → Play, then starts the net task.
async fn handle_login(
    mut conn: Connection<basalt_net::connection::Login>,
    addr: SocketAddr,
    state: Arc<ServerState>,
    network_tx: mpsc::UnboundedSender<NetworkInput>,
    game_tx: mpsc::UnboundedSender<GameInput>,
) -> crate::error::Result<()> {
    let (username, player_uuid) = match conn.read_packet().await? {
        ServerboundLoginPacket::LoginStart(login) => {
            log::info!(target: "basalt::connection", "[{addr}] {}: LoginStart", login.username);
            (login.username, login.player_uuid)
        }
        _ => {
            log::warn!(target: "basalt::connection", "[{addr}] Unexpected packet, expected LoginStart");
            return Ok(());
        }
    };

    let success = ClientboundLoginSuccess {
        uuid: player_uuid,
        username: username.clone(),
        properties: vec![],
    };
    log::debug!(target: "basalt::connection", "[{addr}] -> LoginSuccess");
    let conn = conn.send_login_success(&success).await?;
    log::debug!(target: "basalt::connection", "[{addr}] Login → Configuration");

    handle_configuration(
        conn,
        addr,
        &username,
        player_uuid,
        state,
        network_tx,
        game_tx,
    )
    .await
}

/// Handles Configuration, then creates channels and starts the net task.
async fn handle_configuration(
    mut conn: Connection<basalt_net::connection::Configuration>,
    addr: SocketAddr,
    username: &str,
    player_uuid: basalt_types::Uuid,
    state: Arc<ServerState>,
    network_tx: mpsc::UnboundedSender<NetworkInput>,
    game_tx: mpsc::UnboundedSender<GameInput>,
) -> crate::error::Result<()> {
    // Fetch skin in parallel with registry exchange
    let skin_username = username.to_string();
    let skin_task =
        tokio::spawn(async move { crate::skin::fetch_skin_properties(&skin_username).await });

    let registries = build_default_registries();
    for reg in &registries {
        conn.write_packet_typed(ClientboundConfigurationRegistryData::PACKET_ID, reg)
            .await?;
    }

    let conn = conn.finish_configuration().await?;
    log::debug!(target: "basalt::connection", "[{addr}] Configuration → Play");

    let skin_properties = skin_task.await.unwrap_or_default();
    let entity_id = state.next_entity_id();
    let spawn_y = state.world.spawn_y();

    // Create per-player output channel
    let (output_tx, output_rx) = player_output_channel();

    let position = (0.0, spawn_y, 0.0);
    let username_owned = username.to_string();

    // Notify the network loop
    let _ = network_tx.send(NetworkInput::PlayerConnected {
        entity_id,
        uuid: player_uuid,
        username: username_owned.clone(),
        skin_properties: skin_properties.clone(),
        position,
        yaw: 0.0,
        pitch: 0.0,
        output_tx: output_tx.clone(),
    });

    // Notify the game loop
    let _ = game_tx.send(GameInput::PlayerConnected {
        entity_id,
        uuid: player_uuid,
        username: username_owned.clone(),
        position,
        output_tx: output_tx.clone(),
    });

    log::info!(target: "basalt::connection", "[{addr}] {username} joined (entity {entity_id}), starting net task");

    // Run the net task — blocks until disconnect
    let result = crate::net_task::run_net_task(
        conn,
        addr,
        crate::net_task::NetTaskConfig {
            uuid: player_uuid,
            username: username_owned.clone(),
            network_tx: network_tx.clone(),
            game_tx: game_tx.clone(),
        },
        output_rx,
        &state.command_args,
    )
    .await;

    // Notify both loops of disconnection
    let _ = network_tx.send(NetworkInput::PlayerDisconnected {
        uuid: player_uuid,
        entity_id,
        username: username_owned,
    });
    let _ = game_tx.send(GameInput::PlayerDisconnected { uuid: player_uuid });

    result
}
