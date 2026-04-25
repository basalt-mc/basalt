//! E2E tests for the chunk-batch streaming flow (issue #173).
//!
//! Covers the `ChunkBatchStart` → `MapChunk` × N → `ChunkBatchFinished(N)`
//! protocol contract: every burst is wrapped in matched markers, the
//! declared `batch_size` matches the actual chunk count on the wire,
//! and the full view radius is delivered across multiple batches at
//! the configured initial rate.

use super::*;

/// Connects a client through to Play state and verifies the structure
/// of the initial chunk burst on the wire.
///
/// This is the load-bearing test for #173 — without rate control, the
/// server would emit one giant batch (121 chunks) which is exactly the
/// flooding behavior we ship to fix. The assertions encode three
/// invariants:
/// 1. Every `ChunkBatchStart` is followed by a `ChunkBatchFinished`
///    before another `Start` (no nested batches).
/// 2. The `batch_size` field of `Finished` matches the number of
///    `MapChunk` packets between the markers (no off-by-one).
/// 3. The full view radius (`(2 * VIEW_RADIUS + 1)^2 = 121` chunks)
///    is delivered across multiple batches at the default rate.
#[tokio::test]
async fn e2e_initial_chunks_arrive_in_rate_limited_batches() {
    use basalt_protocol::packets::configuration::{
        ClientboundConfigurationFinishConfiguration, ServerboundConfigurationFinishConfiguration,
    };
    use basalt_protocol::packets::login::{
        ServerboundLoginLoginAcknowledged, ServerboundLoginLoginStart,
    };
    use basalt_protocol::packets::play::world::{
        ClientboundPlayChunkBatchFinished, ClientboundPlayChunkBatchStart, ClientboundPlayMapChunk,
    };

    let addr = spawn_server().await;
    let mut client = TcpStream::connect(addr).await.unwrap();
    client_handshake(&mut client, addr.port(), 2).await;

    // Login
    send_packet(
        &mut client,
        ServerboundLoginLoginStart::PACKET_ID,
        &ServerboundLoginLoginStart {
            username: "Batcher".into(),
            player_uuid: Uuid::default(),
        },
    )
    .await;
    let _: (_, ClientboundLoginSuccess) = recv_packet(&mut client).await;
    send_packet(
        &mut client,
        ServerboundLoginLoginAcknowledged::PACKET_ID,
        &ServerboundLoginLoginAcknowledged,
    )
    .await;

    // Drain Configuration packets up to FinishConfiguration.
    loop {
        let raw = framing::read_raw_packet(&mut client)
            .await
            .unwrap()
            .unwrap();
        if raw.id == ClientboundConfigurationFinishConfiguration::PACKET_ID {
            break;
        }
    }
    send_packet(
        &mut client,
        ServerboundConfigurationFinishConfiguration::PACKET_ID,
        &ServerboundConfigurationFinishConfiguration,
    )
    .await;

    // Drain Play setup up to and including the welcome SystemChat.
    // After the welcome, only chunk batches should appear on the wire
    // until the queue empties.
    loop {
        let raw = framing::read_raw_packet(&mut client)
            .await
            .unwrap()
            .unwrap();
        if raw.id == ClientboundPlaySystemChat::PACKET_ID {
            break;
        }
    }

    // Walk the chunk-batch stream until the server falls silent.
    let mut in_batch = false;
    let mut current_batch_chunks = 0i32;
    let mut completed_batches = 0u32;
    let mut total_map_chunks = 0u32;

    while let Ok(Ok(Some(raw))) = tokio::time::timeout(
        std::time::Duration::from_millis(500),
        framing::read_raw_packet(&mut client),
    )
    .await
    {
        if raw.id == ClientboundPlayChunkBatchStart::PACKET_ID {
            assert!(!in_batch, "ChunkBatchStart received inside an open batch");
            in_batch = true;
            current_batch_chunks = 0;
        } else if raw.id == ClientboundPlayMapChunk::PACKET_ID {
            assert!(in_batch, "MapChunk received outside a batch");
            current_batch_chunks += 1;
            total_map_chunks += 1;
        } else if raw.id == ClientboundPlayChunkBatchFinished::PACKET_ID {
            assert!(
                in_batch,
                "ChunkBatchFinished received without an open batch"
            );
            let mut cursor = raw.payload.as_slice();
            let finished = ClientboundPlayChunkBatchFinished::decode(&mut cursor).unwrap();
            assert_eq!(
                finished.batch_size, current_batch_chunks,
                "Finished.batch_size ({}) must match the {} MapChunk packets in the batch",
                finished.batch_size, current_batch_chunks
            );
            completed_batches += 1;
            in_batch = false;
        }
    }

    assert!(
        !in_batch,
        "stream ended with an open batch (missing ChunkBatchFinished)"
    );
    assert_eq!(
        total_map_chunks, 121,
        "expected 121 chunks for view radius 5 ((2*5+1)^2), got {total_map_chunks}"
    );
    // 121 chunks at the default 25 chunks/tick → ⌈121/25⌉ = 5 batches.
    // Asserting >= 2 keeps the test resilient to default-rate tweaks while
    // still proving the flow is split across multiple ticks.
    assert!(
        completed_batches >= 2,
        "expected the initial burst to span multiple batches (got {completed_batches}); \
         a single giant batch is exactly the flooding behavior #173 prevents"
    );
}
