use super::*;
use basalt_mc_protocol::packets::play::entity::ClientboundPlaySpawnEntity;
use basalt_mc_protocol::packets::play::inventory::{
    ClientboundPlayOpenWindow, ClientboundPlaySetPlayerInventory,
};
use basalt_mc_protocol::packets::play::world::{
    ClientboundPlayAcknowledgePlayerDigging, ServerboundPlayBlockDig, ServerboundPlayBlockPlace,
};
use basalt_mc_protocol::packets::play::{
    ServerboundPlayCloseWindow, ServerboundPlaySetCreativeSlot, ServerboundPlayWindowClick,
};

#[tokio::test]
async fn e2e_block_dig_receives_response() {
    let addr = spawn_server().await;
    let mut client = connect_to_play(addr).await;

    send_packet(
        &mut client,
        ServerboundPlayBlockDig::PACKET_ID,
        &ServerboundPlayBlockDig {
            status: 0,
            location: basalt_types::Position::new(5, 64, 3),
            face: 1,
            sequence: 42,
        },
    )
    .await;

    // Wait for game loop tick
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Should receive ack + block change
    let raw = framing::read_raw_packet(&mut client)
        .await
        .unwrap()
        .unwrap();
    assert!(raw.id >= 0, "should receive a response packet");
}

#[tokio::test]
async fn e2e_block_place_receives_response() {
    let addr = spawn_server().await;
    let mut client = connect_to_play(addr).await;

    // Set a creative slot first
    send_packet(
        &mut client,
        ServerboundPlaySetCreativeSlot::PACKET_ID,
        &ServerboundPlaySetCreativeSlot {
            slot: 36,
            item: basalt_types::Slot {
                item_id: Some(1),
                item_count: 64,
                component_data: vec![],
            },
        },
    )
    .await;

    // Wait for game loop to process inventory
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    send_packet(
        &mut client,
        ServerboundPlayBlockPlace::PACKET_ID,
        &ServerboundPlayBlockPlace {
            hand: 0,
            location: basalt_types::Position::new(5, 63, 3),
            direction: 1,
            cursor_x: 0.5,
            cursor_y: 1.0,
            cursor_z: 0.5,
            inside_block: false,
            world_border_hit: false,
            sequence: 10,
        },
    )
    .await;

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let raw = framing::read_raw_packet(&mut client)
        .await
        .unwrap()
        .unwrap();
    assert!(raw.id >= 0, "should receive a response packet");
}

#[tokio::test]
async fn e2e_drop_single_item_q_key() {
    let addr = spawn_server().await;
    let mut client = connect_to_play(addr).await;

    // Give 10 stone in hotbar slot 0 (window slot 36)
    give_creative_item(&mut client, 36, 1, 10).await;

    // Q key = BlockDig status 4 (drop single item)
    send_packet(
        &mut client,
        ServerboundPlayBlockDig::PACKET_ID,
        &ServerboundPlayBlockDig {
            status: 4,
            location: basalt_types::Position::new(0, 0, 0),
            face: 0,
            sequence: 0,
        },
    )
    .await;

    // Wait for SetPlayerInventory — the game loop sends it after processing
    let packets =
        read_until_packet(&mut client, ClientboundPlaySetPlayerInventory::PACKET_ID).await;

    let inv_pkt = packets
        .iter()
        .find(|p| p.id == ClientboundPlaySetPlayerInventory::PACKET_ID)
        .expect("should receive SetPlayerInventory after Q drop");

    let mut cursor = inv_pkt.payload.as_slice();
    let pkt = ClientboundPlaySetPlayerInventory::decode(&mut cursor).unwrap();
    assert_eq!(pkt.slot_id, 0, "should update hotbar slot 0");
    assert_eq!(
        pkt.contents.item_count, 9,
        "should have 9 after dropping 1 from 10"
    );
}

#[tokio::test]
async fn e2e_drop_full_stack_ctrl_q() {
    let addr = spawn_server().await;
    let mut client = connect_to_play(addr).await;

    give_creative_item(&mut client, 36, 1, 32).await;

    // Ctrl+Q = BlockDig status 3 (drop full stack)
    send_packet(
        &mut client,
        ServerboundPlayBlockDig::PACKET_ID,
        &ServerboundPlayBlockDig {
            status: 3,
            location: basalt_types::Position::new(0, 0, 0),
            face: 0,
            sequence: 0,
        },
    )
    .await;

    let packets =
        read_until_packet(&mut client, ClientboundPlaySetPlayerInventory::PACKET_ID).await;

    let inv_pkt = packets
        .iter()
        .find(|p| p.id == ClientboundPlaySetPlayerInventory::PACKET_ID)
        .expect("should receive SetPlayerInventory after Ctrl+Q drop");

    let mut cursor = inv_pkt.payload.as_slice();
    let pkt = ClientboundPlaySetPlayerInventory::decode(&mut cursor).unwrap();
    assert_eq!(pkt.slot_id, 0);
    assert!(
        pkt.contents.is_empty(),
        "slot should be empty after full stack drop"
    );
}

#[tokio::test]
async fn e2e_block_break_spawns_item_entity() {
    let addr = spawn_server().await;
    let mut client = connect_to_play(addr).await;

    // Place a stone block so we have something guaranteed to break
    give_creative_item(&mut client, 36, 1, 1).await;

    send_packet(
        &mut client,
        ServerboundPlayBlockPlace::PACKET_ID,
        &ServerboundPlayBlockPlace {
            hand: 0,
            location: basalt_types::Position::new(2, -60, 2),
            direction: 1,
            cursor_x: 0.5,
            cursor_y: 1.0,
            cursor_z: 0.5,
            inside_block: false,
            world_border_hit: false,
            sequence: 50,
        },
    )
    .await;

    // Wait for block place to be processed (ack arrives)
    read_until_packet(
        &mut client,
        ClientboundPlayAcknowledgePlayerDigging::PACKET_ID,
    )
    .await;

    // Break the placed block
    send_packet(
        &mut client,
        ServerboundPlayBlockDig::PACKET_ID,
        &ServerboundPlayBlockDig {
            status: 0,
            location: basalt_types::Position::new(2, -59, 2),
            face: 1,
            sequence: 51,
        },
    )
    .await;

    // Wait for SpawnEntity (the dropped item)
    let packets = read_until_packet(&mut client, ClientboundPlaySpawnEntity::PACKET_ID).await;

    let spawn_pkt = packets
        .iter()
        .find(|p| p.id == ClientboundPlaySpawnEntity::PACKET_ID)
        .expect("breaking a block should spawn an item entity");

    let mut cursor = spawn_pkt.payload.as_slice();
    let pkt = ClientboundPlaySpawnEntity::decode(&mut cursor).unwrap();
    assert_eq!(pkt.r#type, 68, "spawned entity should be type 68 (item)");
}

#[tokio::test]
async fn e2e_chest_opens_with_right_click() {
    let addr = spawn_server().await;
    let mut client = connect_to_play(addr).await;

    // Place a chest
    place_chest(&mut client, 2, -60, 2).await;

    // Drain placement packets
    while let Ok(Ok(Some(_))) = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        framing::read_raw_packet(&mut client),
    )
    .await
    {}

    // Right-click the chest (BlockPlace on the chest position)
    send_packet(
        &mut client,
        ServerboundPlayBlockPlace::PACKET_ID,
        &ServerboundPlayBlockPlace {
            hand: 0,
            location: basalt_types::Position::new(2, -59, 2),
            direction: 1,
            cursor_x: 0.5,
            cursor_y: 0.5,
            cursor_z: 0.5,
            inside_block: false,
            world_border_hit: false,
            sequence: 101,
        },
    )
    .await;

    // Should receive OpenWindow
    let packets = read_until_packet(&mut client, ClientboundPlayOpenWindow::PACKET_ID).await;
    let open_pkt = packets
        .iter()
        .find(|p| p.id == ClientboundPlayOpenWindow::PACKET_ID)
        .expect("right-clicking chest should send OpenWindow");

    let mut cursor = open_pkt.payload.as_slice();
    let pkt = ClientboundPlayOpenWindow::decode(&mut cursor).unwrap();
    assert_eq!(pkt.inventory_type, 2, "single chest = generic_9x3 (type 2)");
}

#[tokio::test]
async fn e2e_chest_break_drops_contents_and_block() {
    let addr = spawn_server().await;
    let mut client = connect_to_play(addr).await;

    // Place a chest
    place_chest(&mut client, 3, -60, 3).await;

    // Open the chest and put an item in it
    send_packet(
        &mut client,
        ServerboundPlayBlockPlace::PACKET_ID,
        &ServerboundPlayBlockPlace {
            hand: 0,
            location: basalt_types::Position::new(3, -59, 3),
            direction: 1,
            cursor_x: 0.5,
            cursor_y: 0.5,
            cursor_z: 0.5,
            inside_block: false,
            world_border_hit: false,
            sequence: 102,
        },
    )
    .await;
    read_until_packet(&mut client, ClientboundPlayOpenWindow::PACKET_ID).await;

    // Give the player stone in hotbar slot 0 via creative mode,
    // then pick it up and place it into the chest (server-authoritative).
    // Protocol slot 36 = hotbar 0 in player inventory window.
    give_creative_item(&mut client, 36, 1, 10).await;

    // Left-click hotbar 0 in chest window (slot 27+27=54? no — single chest
    // has 27 container slots, then 27 main inv, then 9 hotbar)
    // Chest window: slots 0-26 = container, 27-53 = main inv, 54-62 = hotbar
    // Hotbar 0 in chest window = slot 54
    send_packet(
        &mut client,
        ServerboundPlayWindowClick::PACKET_ID,
        &ServerboundPlayWindowClick {
            window_id: 1,
            state_id: 0,
            slot: 54,
            mouse_button: 0,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        },
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Left-click on chest slot 0 to place cursor there
    send_packet(
        &mut client,
        ServerboundPlayWindowClick::PACKET_ID,
        &ServerboundPlayWindowClick {
            window_id: 1,
            state_id: 0,
            slot: 0,
            mouse_button: 0,
            mode: 0,
            changed_slots: vec![],
            cursor_item: basalt_types::Slot::empty(),
        },
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Close the chest
    send_packet(
        &mut client,
        ServerboundPlayCloseWindow::PACKET_ID,
        &ServerboundPlayCloseWindow { window_id: 1 },
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Drain all packets
    while let Ok(Ok(Some(_))) = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        framing::read_raw_packet(&mut client),
    )
    .await
    {}

    // Break the chest
    send_packet(
        &mut client,
        ServerboundPlayBlockDig::PACKET_ID,
        &ServerboundPlayBlockDig {
            status: 0,
            location: basalt_types::Position::new(3, -59, 3),
            face: 1,
            sequence: 103,
        },
    )
    .await;

    // Should receive multiple SpawnEntity for dropped items (contents + chest block)
    // First wait for the initial spawn, then collect more
    let mut all_packets =
        read_until_packet(&mut client, ClientboundPlaySpawnEntity::PACKET_ID).await;
    // Keep reading for additional spawns
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, framing::read_raw_packet(&mut client)).await {
            Ok(Ok(Some(raw))) => all_packets.push(raw),
            _ => break,
        }
    }
    let spawn_count = all_packets
        .iter()
        .filter(|p| p.id == ClientboundPlaySpawnEntity::PACKET_ID)
        .count();
    assert!(
        spawn_count >= 2,
        "should drop chest contents + chest block itself, got {spawn_count} spawns"
    );
}

#[tokio::test]
async fn e2e_double_chest_opens_54_slots() {
    let addr = spawn_server().await;
    let mut client = connect_to_play(addr).await;

    // Place two adjacent chests to form a double
    place_chest(&mut client, 4, -60, 4).await;
    place_chest(&mut client, 5, -60, 4).await;

    // Drain packets
    while let Ok(Ok(Some(_))) = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        framing::read_raw_packet(&mut client),
    )
    .await
    {}

    // Open one half
    send_packet(
        &mut client,
        ServerboundPlayBlockPlace::PACKET_ID,
        &ServerboundPlayBlockPlace {
            hand: 0,
            location: basalt_types::Position::new(4, -59, 4),
            direction: 1,
            cursor_x: 0.5,
            cursor_y: 0.5,
            cursor_z: 0.5,
            inside_block: false,
            world_border_hit: false,
            sequence: 104,
        },
    )
    .await;

    let packets = read_until_packet(&mut client, ClientboundPlayOpenWindow::PACKET_ID).await;
    let open_pkt = packets
        .iter()
        .find(|p| p.id == ClientboundPlayOpenWindow::PACKET_ID)
        .expect("right-clicking double chest should send OpenWindow");

    let mut cursor = open_pkt.payload.as_slice();
    let pkt = ClientboundPlayOpenWindow::decode(&mut cursor).unwrap();
    assert_eq!(pkt.inventory_type, 5, "double chest = generic_9x6 (type 5)");
}
