//! Per-player connection handler.
//!
//! Manages the full lifecycle of a client connection: Handshake →
//! Status or Login → Configuration → Play. Each state transition
//! consumes the connection and produces a new one in the target state.
//!
//! The shared `ServerState` is threaded through to the play loop
//! where it is used for player registration and broadcast.

use std::net::SocketAddr;
use std::sync::Arc;

use basalt_net::connection::{Connection, Handshake, HandshakeResult};
use basalt_protocol::packets::configuration::ClientboundConfigurationRegistryData;
use basalt_protocol::packets::login::{ClientboundLoginSuccess, ServerboundLoginPacket};
use basalt_protocol::packets::status::{
    ClientboundStatusPing, ClientboundStatusServerInfo, ServerboundStatusPacket,
};
use basalt_protocol::registry_data::build_default_registries;

use crate::play::run_play_loop;
use crate::player::PlayerState;
use crate::state::{BroadcastMessage, PlayerHandle, PlayerSnapshot, ServerState};

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
/// a login attempt, then delegates to the appropriate handler.
pub(crate) async fn handle_connection(
    stream: tokio::net::TcpStream,
    addr: SocketAddr,
    state: Arc<ServerState>,
) -> crate::error::Result<()> {
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
            handle_login(conn, addr, state).await
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
    state: Arc<ServerState>,
) -> crate::error::Result<()> {
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

    let success = ClientboundLoginSuccess {
        uuid: player_uuid,
        username: username.clone(),
        properties: vec![],
    };
    println!("[{addr}] -> LoginSuccess");
    let conn = conn.send_login_success(&success).await?;
    println!("[{addr}] <- LoginAcknowledged → Configuration");

    handle_configuration(conn, addr, &username, player_uuid, state).await
}

/// Handles the Configuration state: sends all required registries,
/// then sends FinishConfiguration and transitions to Play.
async fn handle_configuration(
    mut conn: Connection<basalt_net::connection::Configuration>,
    addr: SocketAddr,
    username: &str,
    player_uuid: basalt_types::Uuid,
    state: Arc<ServerState>,
) -> crate::error::Result<()> {
    // Start skin fetch in parallel — runs during registry exchange
    // and FinishConfiguration handshake so it doesn't block the join.
    let skin_username = username.to_string();
    let skin_task =
        tokio::spawn(async move { crate::skin::fetch_skin_properties(&skin_username).await });

    let registries = build_default_registries();
    for reg in &registries {
        conn.write_packet_typed(ClientboundConfigurationRegistryData::PACKET_ID, reg)
            .await?;
        println!("[{addr}] -> RegistryData ({})", reg.id);
    }

    let conn = conn.finish_configuration().await?;
    println!("[{addr}] <- FinishConfiguration → Play");

    // Collect skin result — the fetch ran during the config exchange
    let skin_properties = skin_task.await.unwrap_or_default();

    let entity_id = state.next_entity_id();
    let mut player = PlayerState::new(
        username.to_string(),
        player_uuid,
        entity_id,
        skin_properties,
    );

    // Subscribe to broadcast channel before registering so we don't
    // miss our own join notification (we filter it in the play loop).
    let broadcast_rx = state.subscribe();

    // Register in the server state — get the list of existing players
    let existing_players = state.register_player(PlayerHandle {
        username: username.to_string(),
        uuid: player_uuid,
        entity_id,
        skin_properties: player.skin_properties.clone(),
    });

    // Notify all players (including ourselves) that we joined.
    // Our play loop will filter out our own join message.
    let snapshot = PlayerSnapshot {
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
    state.broadcast(BroadcastMessage::PlayerJoined { info: snapshot });

    // Run the play loop — this blocks until the player disconnects
    let result = run_play_loop(
        conn,
        addr,
        &mut player,
        &state,
        broadcast_rx,
        &existing_players,
    )
    .await;

    // Unregister and notify others
    state.unregister_player(&player_uuid);
    state.broadcast(BroadcastMessage::PlayerLeft {
        uuid: player_uuid,
        entity_id,
        username: player.username.clone(),
    });

    result
}
