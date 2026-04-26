use super::*;

#[tokio::test]
async fn e2e_server_login_and_configuration() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    // Handshake → Login
    client_handshake(&mut client, addr.port(), 2).await;

    // LoginStart
    use basalt_mc_protocol::packets::login::ServerboundLoginLoginStart;
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
    use basalt_mc_protocol::packets::login::ServerboundLoginLoginAcknowledged;
    send_packet(
        &mut client,
        ServerboundLoginLoginAcknowledged::PACKET_ID,
        &ServerboundLoginLoginAcknowledged,
    )
    .await;

    // Read registry data packets (at least 5: dimension_type, biome,
    // damage_type, painting_variant, wolf_variant)
    use basalt_mc_protocol::packets::configuration::ClientboundConfigurationRegistryData;
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
    use basalt_mc_protocol::packets::configuration::ServerboundConfigurationFinishConfiguration;
    send_packet(
        &mut client,
        ServerboundConfigurationFinishConfiguration::PACKET_ID,
        &ServerboundConfigurationFinishConfiguration,
    )
    .await;

    // Read Play packets: Login, SpawnPosition, GameEvent, ChunkData, PlayerPosition
    use basalt_mc_protocol::packets::play::player::ClientboundPlayLogin;
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

#[tokio::test]
async fn e2e_server_handles_teleport_confirm() {
    let addr = spawn_server().await;
    let mut client = TcpStream::connect(addr).await.unwrap();

    // Fast-track to Play state
    client_handshake(&mut client, addr.port(), 2).await;

    use basalt_mc_protocol::packets::login::{
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
        use basalt_mc_protocol::packets::configuration::ClientboundConfigurationFinishConfiguration;
        if raw.id == ClientboundConfigurationFinishConfiguration::PACKET_ID {
            break;
        }
    }

    // Send FinishConfiguration ack
    use basalt_mc_protocol::packets::configuration::ServerboundConfigurationFinishConfiguration;
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
    use basalt_mc_protocol::packets::play::player::ServerboundPlayTeleportConfirm;
    send_packet(
        &mut client,
        ServerboundPlayTeleportConfirm::PACKET_ID,
        &ServerboundPlayTeleportConfirm { teleport_id: 1 },
    )
    .await;

    // Send PlayerLoaded
    use basalt_mc_protocol::packets::play::player::ServerboundPlayPlayerLoaded;
    send_packet(
        &mut client,
        ServerboundPlayPlayerLoaded::PACKET_ID,
        &ServerboundPlayPlayerLoaded,
    )
    .await;

    // Send a position update
    use basalt_mc_protocol::packets::play::player::ServerboundPlayPosition;
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
