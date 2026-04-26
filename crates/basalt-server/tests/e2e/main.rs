//! End-to-end tests for the Basalt server.
//!
//! Spawns the server on a random port and connects a fake client that
//! speaks the Minecraft protocol. Validates the full flow from
//! handshake through play state.

mod blocks;
mod chat;
mod chunks;
mod login;
mod multiplayer;
mod status;

use basalt_mc_protocol::packets::handshake::ServerboundHandshakeSetProtocol;
use basalt_mc_protocol::packets::login::ClientboundLoginSuccess;
use basalt_mc_protocol::packets::play::chat::ClientboundPlaySystemChat;
use basalt_net::framing;
use basalt_server::Server;
use basalt_types::{Decode, Encode, EncodedSize, Uuid};
use tokio::net::{TcpListener, TcpStream};

/// Sends a framed packet from the client side.
pub async fn send_packet<P: Encode + EncodedSize>(
    stream: &mut TcpStream,
    packet_id: i32,
    packet: &P,
) {
    let mut payload = Vec::with_capacity(packet.encoded_size());
    packet.encode(&mut payload).unwrap();
    framing::write_raw_packet(stream, packet_id, &payload)
        .await
        .unwrap();
}

/// Reads a framed packet from the client side and decodes it.
pub async fn recv_packet<P: Decode>(stream: &mut TcpStream) -> (i32, P) {
    let raw = framing::read_raw_packet(stream).await.unwrap().unwrap();
    let mut cursor = raw.payload.as_slice();
    let packet = P::decode(&mut cursor).unwrap();
    (raw.id, packet)
}

/// Sends a handshake packet from the client.
pub async fn client_handshake(stream: &mut TcpStream, port: u16, next_state: i32) {
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
pub async fn spawn_server() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::accept_loop(listener).await;
    });
    addr
}

/// Helper: connects a client and fast-tracks through to Play state.
/// Returns the client stream positioned right after all initial Play
/// packets have been consumed (Login, SpawnPosition, GameEvent, Chunk,
/// Position, Welcome message).
pub async fn connect_to_play(addr: std::net::SocketAddr) -> TcpStream {
    connect_to_play_as(addr, "ChatTester", Uuid::default()).await
}

/// Helper: connects a client with a specific username and UUID.
pub async fn connect_to_play_as(
    addr: std::net::SocketAddr,
    username: &str,
    uuid: Uuid,
) -> TcpStream {
    let mut client = TcpStream::connect(addr).await.unwrap();
    client_handshake(&mut client, addr.port(), 2).await;

    use basalt_mc_protocol::packets::login::{
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

    // Drain all initial Play packets until we receive the welcome
    // SystemChat message — it's the last "control" packet sent during
    // the join sequence.
    loop {
        let raw = framing::read_raw_packet(&mut client)
            .await
            .unwrap()
            .unwrap();
        if raw.id == ClientboundPlaySystemChat::PACKET_ID {
            break;
        }
    }

    // After the welcome message the drainer ships exactly 121 chunks
    // (view radius 5 → `(2*5+1)^2`) across several ticks at the
    // default 25 chunks/tick rate. Reading until we observe both
    // (a) all 121 `MapChunk` packets and (b) the trailing
    // `ChunkBatchFinished` of the batch that contained the last chunk
    // is deterministic — no timeout race on slow CI runners — and
    // leaves the wire idle so tests can start their real work.
    use basalt_mc_protocol::packets::play::world::{
        ClientboundPlayChunkBatchFinished, ClientboundPlayMapChunk,
    };
    const INITIAL_CHUNK_COUNT: i32 = 121;
    let mut chunks_drained = 0;
    loop {
        let raw = framing::read_raw_packet(&mut client)
            .await
            .unwrap()
            .unwrap();
        if raw.id == ClientboundPlayMapChunk::PACKET_ID {
            chunks_drained += 1;
        } else if raw.id == ClientboundPlayChunkBatchFinished::PACKET_ID
            && chunks_drained >= INITIAL_CHUNK_COUNT
        {
            break;
        }
    }

    client
}

/// Reads packets from the stream until one with the given `target_id`
/// is found. Returns all packets received (including the target).
/// Uses a 5-second overall timeout — generous enough for any CI runner.
///
/// This replaces sleep-based polling: we block on the TCP read until
/// the game loop has processed the request and sent the response.
pub async fn read_until_packet(
    client: &mut TcpStream,
    target_id: i32,
) -> Vec<basalt_net::framing::RawPacket> {
    let mut collected = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, framing::read_raw_packet(client)).await {
            Ok(Ok(Some(raw))) => {
                let found = raw.id == target_id;
                collected.push(raw);
                if found {
                    break;
                }
            }
            _ => break,
        }
    }
    collected
}

/// Helper: give an item to a connected player via SetCreativeSlot,
/// then wait for the game loop to process it by reading until we
/// see a SetPlayerInventory response confirming the slot was set.
pub async fn give_creative_item(
    client: &mut TcpStream,
    protocol_slot: i16,
    item_id: i32,
    count: i32,
) {
    use basalt_mc_protocol::packets::play::ServerboundPlaySetCreativeSlot;
    send_packet(
        client,
        ServerboundPlaySetCreativeSlot::PACKET_ID,
        &ServerboundPlaySetCreativeSlot {
            slot: protocol_slot,
            item: basalt_types::Slot::new(item_id, count),
        },
    )
    .await;
    // The server doesn't send a response for SetCreativeSlot, so we
    // need a small delay for the game loop to process it.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    // Drain any packets that arrived during the wait
    while let Ok(Ok(Some(_))) = tokio::time::timeout(
        std::time::Duration::from_millis(10),
        framing::read_raw_packet(client),
    )
    .await
    {}
}

/// Places a chest via creative slot + BlockPlace, waits for processing.
pub async fn place_chest(client: &mut TcpStream, x: i32, y: i32, z: i32) {
    use basalt_mc_protocol::packets::play::world::ServerboundPlayBlockPlace;
    // Give chest in hotbar slot 0 (item 313)
    give_creative_item(client, 36, 313, 1).await;
    send_packet(
        client,
        ServerboundPlayBlockPlace::PACKET_ID,
        &ServerboundPlayBlockPlace {
            hand: 0,
            location: basalt_types::Position::new(x, y, z),
            direction: 1, // top face
            cursor_x: 0.5,
            cursor_y: 1.0,
            cursor_z: 0.5,
            inside_block: false,
            world_border_hit: false,
            sequence: 100,
        },
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
}
