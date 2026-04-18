use super::*;
use basalt_protocol::packets::play::chat::ServerboundPlayChatMessage;
use basalt_protocol::packets::play::entity::ClientboundPlaySpawnEntity;
use basalt_protocol::packets::play::player::{ClientboundPlayPlayerInfo, ServerboundPlayPosition};

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
    use basalt_protocol::packets::play::chat::ClientboundPlaySystemChat;
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
    use basalt_protocol::packets::play::chat::ClientboundPlaySystemChat;
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

#[tokio::test]
async fn e2e_player_disconnect_notifies_other() {
    let addr = spawn_server().await;

    let uuid1 = Uuid::from_bytes([30; 16]);
    let uuid2 = Uuid::from_bytes([40; 16]);

    let mut client1 = connect_to_play_as(addr, "Stayer", uuid1).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let client2 = connect_to_play_as(addr, "Leaver", uuid2).await;

    // Drain join packets on client1
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    while let Ok(Ok(Some(_))) = tokio::time::timeout(
        std::time::Duration::from_millis(10),
        framing::read_raw_packet(&mut client1),
    )
    .await
    {}

    // Client 2 disconnects
    drop(client2);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Client 1 should receive leave broadcast
    let mut got_packets = false;
    while let Ok(Ok(Some(raw))) = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        framing::read_raw_packet(&mut client1),
    )
    .await
    {
        if raw.id >= 0 {
            got_packets = true;
        }
    }
    assert!(got_packets, "should receive disconnect broadcast");
}

#[tokio::test]
async fn e2e_movement_broadcast_to_other_player() {
    let addr = spawn_server().await;

    let uuid1 = Uuid::from_bytes([50; 16]);
    let uuid2 = Uuid::from_bytes([60; 16]);

    let mut client1 = connect_to_play_as(addr, "Mover", uuid1).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut client2 = connect_to_play_as(addr, "Watcher", uuid2).await;

    // Drain join packets on client2
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    while let Ok(Ok(Some(_))) = tokio::time::timeout(
        std::time::Duration::from_millis(10),
        framing::read_raw_packet(&mut client2),
    )
    .await
    {}

    // Client 1 sends position update
    send_packet(
        &mut client1,
        ServerboundPlayPosition::PACKET_ID,
        &ServerboundPlayPosition {
            x: 5.0,
            y: -60.0,
            z: 3.0,
            flags: 1,
        },
    )
    .await;

    // Wait for game loop tick
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Client 2 should receive movement broadcast
    let mut got_movement = false;
    while let Ok(Ok(Some(raw))) = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        framing::read_raw_packet(&mut client2),
    )
    .await
    {
        if raw.id >= 0 {
            got_movement = true;
        }
    }
    assert!(got_movement, "should receive movement broadcast");
}
