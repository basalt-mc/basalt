use super::*;
use basalt_mc_protocol::packets::play::chat::{
    ClientboundPlaySystemChat, ServerboundPlayChatCommand, ServerboundPlayChatMessage,
};
use basalt_mc_protocol::packets::play::player::{
    ClientboundPlayGameStateChange, ClientboundPlayPosition,
};

#[tokio::test]
async fn e2e_server_chat_message_echoed() {
    let addr = spawn_server().await;
    let mut client = connect_to_play(addr).await;

    // Send a chat message
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
    let (id, _response): (_, ClientboundPlaySystemChat) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundPlaySystemChat::PACKET_ID);
    // The response contains an NbtCompound with the formatted message
}

#[tokio::test]
async fn e2e_server_command_help() {
    let addr = spawn_server().await;
    let mut client = connect_to_play(addr).await;

    // Send /help command
    send_packet(
        &mut client,
        ServerboundPlayChatCommand::PACKET_ID,
        &ServerboundPlayChatCommand {
            command: "help".into(),
        },
    )
    .await;

    // Read the SystemChat response with help text
    let (id, _response): (_, ClientboundPlaySystemChat) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundPlaySystemChat::PACKET_ID);
}

#[tokio::test]
async fn e2e_server_command_unknown() {
    let addr = spawn_server().await;
    let mut client = connect_to_play(addr).await;

    // Send unknown command
    send_packet(
        &mut client,
        ServerboundPlayChatCommand::PACKET_ID,
        &ServerboundPlayChatCommand {
            command: "doesnotexist".into(),
        },
    )
    .await;

    // Read error response
    let (id, _response): (_, ClientboundPlaySystemChat) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundPlaySystemChat::PACKET_ID);
}

#[tokio::test]
async fn e2e_server_command_say() {
    let addr = spawn_server().await;
    let mut client = connect_to_play(addr).await;

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
