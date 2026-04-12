//! End-to-end tests for the Basalt server.
//!
//! Spawns the server on a random port and connects a fake client that
//! speaks the Minecraft protocol. Validates the full flow from
//! handshake through play state.

use basalt_net::framing;
use basalt_protocol::packets::handshake::ServerboundHandshakeSetProtocol;
use basalt_protocol::packets::login::ClientboundLoginSuccess;
use basalt_protocol::packets::play::chat::ClientboundPlaySystemChat;
use basalt_protocol::packets::status::{
    ClientboundStatusPing, ClientboundStatusServerInfo, ServerboundStatusPing,
    ServerboundStatusPingStart,
};
use basalt_server::Server;
use basalt_types::{Decode, Encode, EncodedSize, Uuid};
use tokio::net::{TcpListener, TcpStream};

/// Sends a framed packet from the client side.
async fn send_packet<P: Encode + EncodedSize>(stream: &mut TcpStream, packet_id: i32, packet: &P) {
    let mut payload = Vec::with_capacity(packet.encoded_size());
    packet.encode(&mut payload).unwrap();
    framing::write_raw_packet(stream, packet_id, &payload)
        .await
        .unwrap();
}

/// Reads a framed packet from the client side and decodes it.
async fn recv_packet<P: Decode>(stream: &mut TcpStream) -> (i32, P) {
    let raw = framing::read_raw_packet(stream).await.unwrap().unwrap();
    let mut cursor = raw.payload.as_slice();
    let packet = P::decode(&mut cursor).unwrap();
    (raw.id, packet)
}

/// Sends a handshake packet from the client.
async fn client_handshake(stream: &mut TcpStream, port: u16, next_state: i32) {
    let handshake = ServerboundHandshakeSetProtocol {
        protocol_version: 769,
        server_host: "localhost".into(),
        server_port: port,
        next_state,
    };
    send_packet(
        stream,
        ServerboundHandshakeSetProtocol::PACKET_ID,
        &handshake,
    )
    .await;
}

/// Spawns a server on a random port and returns the listener address.
/// The server runs in a background task.
async fn spawn_server() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::accept_loop(listener).await;
    });
    addr
}

// -- Status tests --

#[tokio::test]
async fn e2e_server_status_ping() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    // Handshake → Status
    client_handshake(&mut client, addr.port(), 1).await;

    // StatusRequest
    send_packet(
        &mut client,
        ServerboundStatusPingStart::PACKET_ID,
        &ServerboundStatusPingStart,
    )
    .await;

    // Read StatusResponse
    let (id, response): (_, ClientboundStatusServerInfo) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundStatusServerInfo::PACKET_ID);
    assert!(response.response.contains("Basalt"));
    assert!(response.response.contains("769"));

    // Ping/Pong
    send_packet(
        &mut client,
        ServerboundStatusPing::PACKET_ID,
        &ServerboundStatusPing { time: 123 },
    )
    .await;

    let (id, pong): (_, ClientboundStatusPing) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundStatusPing::PACKET_ID);
    assert_eq!(pong.time, 123);
}

// -- Login tests --

#[tokio::test]
async fn e2e_server_login_and_configuration() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    // Handshake → Login
    client_handshake(&mut client, addr.port(), 2).await;

    // LoginStart
    use basalt_protocol::packets::login::ServerboundLoginLoginStart;
    let login_start = ServerboundLoginLoginStart {
        username: "TestPlayer".into(),
        player_uuid: Uuid::default(),
    };
    send_packet(
        &mut client,
        ServerboundLoginLoginStart::PACKET_ID,
        &login_start,
    )
    .await;

    // Read LoginSuccess
    let (id, success): (_, ClientboundLoginSuccess) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundLoginSuccess::PACKET_ID);
    assert_eq!(success.username, "TestPlayer");

    // Send LoginAcknowledged
    use basalt_protocol::packets::login::ServerboundLoginLoginAcknowledged;
    send_packet(
        &mut client,
        ServerboundLoginLoginAcknowledged::PACKET_ID,
        &ServerboundLoginLoginAcknowledged,
    )
    .await;

    // Read registry data packets (at least 5: dimension_type, biome,
    // damage_type, painting_variant, wolf_variant)
    use basalt_protocol::packets::configuration::ClientboundConfigurationRegistryData;
    let mut registry_count = 0;
    loop {
        let raw = framing::read_raw_packet(&mut client)
            .await
            .unwrap()
            .unwrap();
        if raw.id == ClientboundConfigurationRegistryData::PACKET_ID {
            registry_count += 1;
        } else {
            // FinishConfiguration packet
            break;
        }
    }
    assert!(
        registry_count >= 5,
        "expected at least 5 registries, got {registry_count}"
    );

    // Send FinishConfiguration acknowledged
    use basalt_protocol::packets::configuration::ServerboundConfigurationFinishConfiguration;
    send_packet(
        &mut client,
        ServerboundConfigurationFinishConfiguration::PACKET_ID,
        &ServerboundConfigurationFinishConfiguration,
    )
    .await;

    // Read Play packets: Login, SpawnPosition, GameEvent, ChunkData, PlayerPosition
    use basalt_protocol::packets::play::player::ClientboundPlayLogin;
    let (id, login): (_, ClientboundPlayLogin) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundPlayLogin::PACKET_ID);
    assert_eq!(login.entity_id, 1);
    assert_eq!(login.world_state.gamemode, 1); // creative

    // Read remaining initial packets (spawn position, game event, chunk, position)
    // We just verify they arrive without errors
    for _ in 0..4 {
        let raw = framing::read_raw_packet(&mut client)
            .await
            .unwrap()
            .unwrap();
        assert!(raw.id >= 0, "expected valid packet id");
    }
}

// -- Play packet dispatch test --

#[tokio::test]
async fn e2e_server_handles_teleport_confirm() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    // Fast-track to Play state
    client_handshake(&mut client, addr.port(), 2).await;

    use basalt_protocol::packets::login::{
        ServerboundLoginLoginAcknowledged, ServerboundLoginLoginStart,
    };
    send_packet(
        &mut client,
        ServerboundLoginLoginStart::PACKET_ID,
        &ServerboundLoginLoginStart {
            username: "Tester".into(),
            player_uuid: Uuid::default(),
        },
    )
    .await;

    // LoginSuccess
    let _: (_, ClientboundLoginSuccess) = recv_packet(&mut client).await;

    // LoginAcknowledged
    send_packet(
        &mut client,
        ServerboundLoginLoginAcknowledged::PACKET_ID,
        &ServerboundLoginLoginAcknowledged,
    )
    .await;

    // Read all Configuration packets until FinishConfiguration
    loop {
        let raw = framing::read_raw_packet(&mut client)
            .await
            .unwrap()
            .unwrap();
        use basalt_protocol::packets::configuration::ClientboundConfigurationFinishConfiguration;
        if raw.id == ClientboundConfigurationFinishConfiguration::PACKET_ID {
            break;
        }
    }

    // Send FinishConfiguration ack
    use basalt_protocol::packets::configuration::ServerboundConfigurationFinishConfiguration;
    send_packet(
        &mut client,
        ServerboundConfigurationFinishConfiguration::PACKET_ID,
        &ServerboundConfigurationFinishConfiguration,
    )
    .await;

    // Read all initial Play packets (Login, SpawnPosition, GameEvent, Chunk, Position, Welcome)
    for _ in 0..6 {
        framing::read_raw_packet(&mut client)
            .await
            .unwrap()
            .unwrap();
    }

    // Now in Play state — send TeleportConfirm
    use basalt_protocol::packets::play::player::ServerboundPlayTeleportConfirm;
    send_packet(
        &mut client,
        ServerboundPlayTeleportConfirm::PACKET_ID,
        &ServerboundPlayTeleportConfirm { teleport_id: 1 },
    )
    .await;

    // Send PlayerLoaded
    use basalt_protocol::packets::play::player::ServerboundPlayPlayerLoaded;
    send_packet(
        &mut client,
        ServerboundPlayPlayerLoaded::PACKET_ID,
        &ServerboundPlayPlayerLoaded,
    )
    .await;

    // Send a position update
    use basalt_protocol::packets::play::player::ServerboundPlayPosition;
    send_packet(
        &mut client,
        ServerboundPlayPosition::PACKET_ID,
        &ServerboundPlayPosition {
            x: 10.0,
            y: 65.0,
            z: -5.0,
            flags: 0x01,
        },
    )
    .await;

    // Give the server time to process
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // If we got here without panic or error, the server handled
    // all packets correctly. Drop the client to trigger disconnect.
    drop(client);

    // Small delay to let the server log the disconnect
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
}

/// Helper: connects a client and fast-tracks through to Play state.
/// Returns the client stream positioned right after all initial Play
/// packets have been consumed (Login, SpawnPosition, GameEvent, Chunk,
/// Position, Welcome message).
async fn connect_to_play(addr: std::net::SocketAddr) -> TcpStream {
    connect_to_play_as(addr, "ChatTester", Uuid::default()).await
}

/// Helper: connects a client with a specific username and UUID.
async fn connect_to_play_as(addr: std::net::SocketAddr, username: &str, uuid: Uuid) -> TcpStream {
    let mut client = TcpStream::connect(addr).await.unwrap();
    client_handshake(&mut client, addr.port(), 2).await;

    use basalt_protocol::packets::login::{
        ServerboundLoginLoginAcknowledged, ServerboundLoginLoginStart,
    };
    send_packet(
        &mut client,
        ServerboundLoginLoginStart::PACKET_ID,
        &ServerboundLoginLoginStart {
            username: username.into(),
            player_uuid: uuid,
        },
    )
    .await;

    // LoginSuccess
    let _: (_, ClientboundLoginSuccess) = recv_packet(&mut client).await;

    // LoginAcknowledged
    send_packet(
        &mut client,
        ServerboundLoginLoginAcknowledged::PACKET_ID,
        &ServerboundLoginLoginAcknowledged,
    )
    .await;

    // Read Config packets until FinishConfiguration
    loop {
        let raw = framing::read_raw_packet(&mut client)
            .await
            .unwrap()
            .unwrap();
        use basalt_protocol::packets::configuration::ClientboundConfigurationFinishConfiguration;
        if raw.id == ClientboundConfigurationFinishConfiguration::PACKET_ID {
            break;
        }
    }

    // Send FinishConfiguration ack
    use basalt_protocol::packets::configuration::ServerboundConfigurationFinishConfiguration;
    send_packet(
        &mut client,
        ServerboundConfigurationFinishConfiguration::PACKET_ID,
        &ServerboundConfigurationFinishConfiguration,
    )
    .await;

    // Drain all initial Play packets until we receive the welcome
    // SystemChat message — it's always the last packet sent during
    // the join sequence. This avoids fragile timeout-based draining.
    loop {
        let raw = framing::read_raw_packet(&mut client)
            .await
            .unwrap()
            .unwrap();
        if raw.id == ClientboundPlaySystemChat::PACKET_ID {
            break;
        }
    }

    client
}

// -- Chat tests --

#[tokio::test]
async fn e2e_server_chat_message_echoed() {
    let addr = spawn_server().await;
    let mut client = connect_to_play(addr).await;

    // Send a chat message
    use basalt_protocol::packets::play::chat::ServerboundPlayChatMessage;
    send_packet(
        &mut client,
        ServerboundPlayChatMessage::PACKET_ID,
        &ServerboundPlayChatMessage {
            message: "hello world".into(),
            timestamp: 0,
            salt: 0,
            signature: None,
            offset: 0,
            acknowledged: vec![],
        },
    )
    .await;

    // Read the SystemChat response
    use basalt_protocol::packets::play::chat::ClientboundPlaySystemChat;
    let (id, _response): (_, ClientboundPlaySystemChat) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundPlaySystemChat::PACKET_ID);
    // The response contains an NbtCompound with the formatted message
}

#[tokio::test]
async fn e2e_server_command_help() {
    let addr = spawn_server().await;
    let mut client = connect_to_play(addr).await;

    // Send /help command
    use basalt_protocol::packets::play::chat::ServerboundPlayChatCommand;
    send_packet(
        &mut client,
        ServerboundPlayChatCommand::PACKET_ID,
        &ServerboundPlayChatCommand {
            command: "help".into(),
        },
    )
    .await;

    // Read the SystemChat response with help text
    use basalt_protocol::packets::play::chat::ClientboundPlaySystemChat;
    let (id, _response): (_, ClientboundPlaySystemChat) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundPlaySystemChat::PACKET_ID);
}

#[tokio::test]
async fn e2e_server_command_unknown() {
    let addr = spawn_server().await;
    let mut client = connect_to_play(addr).await;

    // Send unknown command
    use basalt_protocol::packets::play::chat::ServerboundPlayChatCommand;
    send_packet(
        &mut client,
        ServerboundPlayChatCommand::PACKET_ID,
        &ServerboundPlayChatCommand {
            command: "doesnotexist".into(),
        },
    )
    .await;

    // Read error response
    use basalt_protocol::packets::play::chat::ClientboundPlaySystemChat;
    let (id, _response): (_, ClientboundPlaySystemChat) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundPlaySystemChat::PACKET_ID);
}

#[tokio::test]
async fn e2e_server_command_say() {
    let addr = spawn_server().await;
    let mut client = connect_to_play(addr).await;

    use basalt_protocol::packets::play::chat::{
        ClientboundPlaySystemChat, ServerboundPlayChatCommand,
    };
    send_packet(
        &mut client,
        ServerboundPlayChatCommand::PACKET_ID,
        &ServerboundPlayChatCommand {
            command: "say hello everyone".into(),
        },
    )
    .await;

    let (id, _): (_, ClientboundPlaySystemChat) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundPlaySystemChat::PACKET_ID);
}

#[tokio::test]
async fn e2e_server_command_tp() {
    let addr = spawn_server().await;
    let mut client = connect_to_play(addr).await;

    use basalt_protocol::packets::play::chat::{
        ClientboundPlaySystemChat, ServerboundPlayChatCommand,
    };

    // Valid tp
    send_packet(
        &mut client,
        ServerboundPlayChatCommand::PACKET_ID,
        &ServerboundPlayChatCommand {
            command: "tp 10 200 -30".into(),
        },
    )
    .await;

    // Read PlayerPosition packet (teleport) + SystemChat feedback
    use basalt_protocol::packets::play::player::ClientboundPlayPosition;
    let (id, pos): (_, ClientboundPlayPosition) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundPlayPosition::PACKET_ID);
    assert_eq!(pos.x, 10.0);
    assert_eq!(pos.y, 200.0);
    assert_eq!(pos.z, -30.0);

    let (id, _): (_, ClientboundPlaySystemChat) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundPlaySystemChat::PACKET_ID);

    // Invalid tp (wrong args)
    send_packet(
        &mut client,
        ServerboundPlayChatCommand::PACKET_ID,
        &ServerboundPlayChatCommand {
            command: "tp 10".into(),
        },
    )
    .await;

    let (id, _): (_, ClientboundPlaySystemChat) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundPlaySystemChat::PACKET_ID);

    // Invalid tp (bad number)
    send_packet(
        &mut client,
        ServerboundPlayChatCommand::PACKET_ID,
        &ServerboundPlayChatCommand {
            command: "tp abc 0 0".into(),
        },
    )
    .await;

    let (id, _): (_, ClientboundPlaySystemChat) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundPlaySystemChat::PACKET_ID);
}

#[tokio::test]
async fn e2e_server_command_gamemode() {
    let addr = spawn_server().await;
    let mut client = connect_to_play(addr).await;

    use basalt_protocol::packets::play::chat::{
        ClientboundPlaySystemChat, ServerboundPlayChatCommand,
    };

    // Valid gamemode
    send_packet(
        &mut client,
        ServerboundPlayChatCommand::PACKET_ID,
        &ServerboundPlayChatCommand {
            command: "gamemode survival".into(),
        },
    )
    .await;

    // Read GameStateChange + SystemChat feedback
    use basalt_protocol::packets::play::player::ClientboundPlayGameStateChange;
    let (id, event): (_, ClientboundPlayGameStateChange) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundPlayGameStateChange::PACKET_ID);
    assert_eq!(event.reason, 3); // change game mode
    assert_eq!(event.game_mode, 0.0); // survival

    let (id, _): (_, ClientboundPlaySystemChat) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundPlaySystemChat::PACKET_ID);

    // Invalid gamemode
    send_packet(
        &mut client,
        ServerboundPlayChatCommand::PACKET_ID,
        &ServerboundPlayChatCommand {
            command: "gamemode invalid".into(),
        },
    )
    .await;

    let (id, _): (_, ClientboundPlaySystemChat) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundPlaySystemChat::PACKET_ID);
}

// -- Multi-player tests --

#[tokio::test]
async fn e2e_two_players_second_gets_player_info() {
    let addr = spawn_server().await;

    let uuid1 = Uuid::from_bytes([1; 16]);
    let uuid2 = Uuid::from_bytes([2; 16]);

    // Player 1 connects
    let mut client1 = connect_to_play_as(addr, "Alice", uuid1).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Player 2 connects — should receive PlayerInfo for Alice
    let mut client2 = TcpStream::connect(addr).await.unwrap();
    client_handshake(&mut client2, addr.port(), 2).await;

    use basalt_protocol::packets::login::{
        ServerboundLoginLoginAcknowledged, ServerboundLoginLoginStart,
    };
    send_packet(
        &mut client2,
        ServerboundLoginLoginStart::PACKET_ID,
        &ServerboundLoginLoginStart {
            username: "Bob".into(),
            player_uuid: uuid2,
        },
    )
    .await;

    let _: (_, ClientboundLoginSuccess) = recv_packet(&mut client2).await;
    send_packet(
        &mut client2,
        ServerboundLoginLoginAcknowledged::PACKET_ID,
        &ServerboundLoginLoginAcknowledged,
    )
    .await;

    loop {
        let raw = framing::read_raw_packet(&mut client2)
            .await
            .unwrap()
            .unwrap();
        use basalt_protocol::packets::configuration::ClientboundConfigurationFinishConfiguration;
        if raw.id == ClientboundConfigurationFinishConfiguration::PACKET_ID {
            break;
        }
    }

    use basalt_protocol::packets::configuration::ServerboundConfigurationFinishConfiguration;
    send_packet(
        &mut client2,
        ServerboundConfigurationFinishConfiguration::PACKET_ID,
        &ServerboundConfigurationFinishConfiguration,
    )
    .await;

    // Drain initial Play packets until the welcome SystemChat.
    // Track whether we see PlayerInfo and SpawnEntity for Alice.
    let mut found_player_info = false;
    let mut found_spawn_entity = false;
    use basalt_protocol::packets::play::entity::ClientboundPlaySpawnEntity;
    use basalt_protocol::packets::play::player::ClientboundPlayPlayerInfo;
    loop {
        let raw = framing::read_raw_packet(&mut client2)
            .await
            .unwrap()
            .unwrap();
        if raw.id == ClientboundPlayPlayerInfo::PACKET_ID {
            found_player_info = true;
        }
        if raw.id == ClientboundPlaySpawnEntity::PACKET_ID {
            found_spawn_entity = true;
        }
        if raw.id == ClientboundPlaySystemChat::PACKET_ID {
            break;
        }
    }
    assert!(
        found_player_info,
        "client2 should receive PlayerInfo for Alice"
    );
    assert!(
        found_spawn_entity,
        "client2 should receive SpawnEntity for Alice"
    );

    // Client 1 should have received PlayerJoined broadcast
    // (PlayerInfo + SpawnEntity + join msg = 3 packets)
    for _ in 0..3 {
        let raw = framing::read_raw_packet(&mut client1)
            .await
            .unwrap()
            .unwrap();
        assert!(raw.id >= 0);
    }

    drop(client1);
    drop(client2);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
}

#[tokio::test]
async fn e2e_chat_broadcast_to_both_players() {
    let addr = spawn_server().await;

    let uuid1 = Uuid::from_bytes([10; 16]);
    let uuid2 = Uuid::from_bytes([20; 16]);

    let mut client1 = connect_to_play_as(addr, "Player1", uuid1).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut client2 = connect_to_play_as(addr, "Player2", uuid2).await;

    // Drain PlayerJoined packets until the "joined the game" SystemChat
    loop {
        let raw = framing::read_raw_packet(&mut client1)
            .await
            .unwrap()
            .unwrap();
        if raw.id == ClientboundPlaySystemChat::PACKET_ID {
            break;
        }
    }

    // Player 1 sends a chat message
    use basalt_protocol::packets::play::chat::ServerboundPlayChatMessage;
    send_packet(
        &mut client1,
        ServerboundPlayChatMessage::PACKET_ID,
        &ServerboundPlayChatMessage {
            message: "hello from player1".into(),
            timestamp: 0,
            salt: 0,
            signature: None,
            offset: 0,
            acknowledged: vec![],
        },
    )
    .await;

    // Both players should receive the chat via broadcast
    let (id1, _): (_, ClientboundPlaySystemChat) = recv_packet(&mut client1).await;
    assert_eq!(id1, ClientboundPlaySystemChat::PACKET_ID);

    let (id2, _): (_, ClientboundPlaySystemChat) = recv_packet(&mut client2).await;
    assert_eq!(id2, ClientboundPlaySystemChat::PACKET_ID);

    drop(client1);
    drop(client2);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
}
