//! Per-player net task -- TCP I/O, instant events, and game loop forwarding.
//!
//! Each connected player has one net task running as a tokio task.
//! The net task:
//! 1. Reads packets from the TCP connection
//! 2. Handles instant events (chat, commands) via `Arc<EventBus>` dispatch
//! 3. Forwards game-relevant packets (movement, blocks, inventory) to the game loop
//! 4. Receives [`ServerOutput`] game events, encodes protocol packets, and writes to TCP
//! 5. Handles keep-alive and tab-complete inline

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use basalt_api::EventBus;
use basalt_net::connection::{Connection, Play};
use basalt_protocol::packets::play::ServerboundPlayPacket;
use basalt_protocol::packets::play::misc::ClientboundPlayKeepAlive;
use basalt_types::Uuid;
use dashmap::DashMap;
use tokio::sync::{broadcast, mpsc};

use super::{play_handler, play_sender};
use crate::messages::{GameInput, ServerOutput};
use crate::net::chunk_cache::ChunkPacketCache;
use crate::state::CommandMeta;

/// Per-player net task configuration.
pub(crate) struct NetTaskConfig {
    /// Player UUID.
    pub uuid: Uuid,
    /// Player display name.
    pub username: String,
    /// Player entity ID (for context creation).
    pub entity_id: i32,
    /// Sender for the game loop.
    pub game_tx: mpsc::UnboundedSender<GameInput>,
    /// Instant event bus (chat, commands). Shared across all net tasks.
    pub instant_bus: Arc<EventBus>,
    /// Broadcast sender for instant fan-out.
    pub broadcast_tx: broadcast::Sender<ServerOutput>,
    /// Player registry for targeted sending.
    pub player_registry: Arc<DashMap<Uuid, mpsc::Sender<ServerOutput>>>,
    /// Shared world reference for context creation.
    pub world: Arc<basalt_world::World>,
    /// Shared chunk packet cache.
    pub chunk_cache: Arc<ChunkPacketCache>,
    /// Command metadata for tab-complete and /help.
    pub command_args: Vec<CommandMeta>,
    /// Maximum inbound packets per second per player.
    /// Exceeding kicks the connection (DoS mitigation).
    pub max_inbound_packets_per_second: u32,
}

/// Runs the per-player net task until disconnect.
pub(crate) async fn run_net_task(
    mut conn: Connection<Play>,
    addr: SocketAddr,
    config: NetTaskConfig,
    mut output_rx: mpsc::Receiver<ServerOutput>,
) -> crate::error::Result<()> {
    let uuid = config.uuid;
    let username = config.username;
    let entity_id = config.entity_id;
    let game_tx = config.game_tx;
    let instant_bus = config.instant_bus;
    let broadcast_tx = config.broadcast_tx;
    let player_registry = config.player_registry;
    let world = config.world;
    // chunk_cache is no longer used by the net task — chunk encoding
    // happens at construction time in `helpers::send_chunk_with_entities`
    // and the bytes flow through `ServerOutput::RawBorrowed`.
    let _chunk_cache = config.chunk_cache;
    let command_args = config.command_args;
    let max_inbound_packets_per_second = config.max_inbound_packets_per_second;

    let mut broadcast_rx = broadcast_tx.subscribe();
    let mut keep_alive = tokio::time::interval(std::time::Duration::from_secs(15));
    keep_alive.tick().await;

    let mut last_keep_alive_id: i64 = 0;
    let mut last_keep_alive_sent = Instant::now();

    // Inbound rate limit — sliding 1-second window; if more than
    // `max_inbound_packets_per_second` packets land in the window,
    // the connection is dropped on the next received packet.
    let mut packet_window_start = Instant::now();
    let mut packet_count_in_window: u32 = 0;

    loop {
        tokio::select! {
            // Branch 1: Keep-alive timer
            _ = keep_alive.tick() => {
                if last_keep_alive_id > 0
                    && last_keep_alive_sent.elapsed() > std::time::Duration::from_secs(30)
                {
                    log::warn!(target: "basalt::net_task", "[{addr}] {username} timed out (no keep-alive response in 30s)");
                    break;
                }
                last_keep_alive_id += 1;
                last_keep_alive_sent = Instant::now();
                let ka = ClientboundPlayKeepAlive {
                    keep_alive_id: last_keep_alive_id,
                };
                conn.write_packet_typed(ClientboundPlayKeepAlive::PACKET_ID, &ka).await?;
            }

            // Branch 2: Read packets from TCP
            result = conn.read_packet() => {
                match result {
                    Ok(packet) => {
                        // Rate limit check: reset the window every second,
                        // count each inbound packet, kick if over budget.
                        if packet_window_start.elapsed() >= std::time::Duration::from_secs(1) {
                            packet_window_start = Instant::now();
                            packet_count_in_window = 0;
                        }
                        packet_count_in_window = packet_count_in_window.saturating_add(1);
                        if packet_count_in_window > max_inbound_packets_per_second {
                            log::warn!(
                                target: "basalt::net_task",
                                "[{addr}] {username} kicked: inbound rate limit exceeded ({packet_count_in_window} packets in <1s, max {max_inbound_packets_per_second})",
                            );
                            break;
                        }

                        if let ServerboundPlayPacket::TabComplete(tc) = &packet {
                            play_handler::handle_tab_complete(&mut conn, &command_args, tc).await?;
                            continue;
                        }
                        play_handler::handle_packet(
                            addr, uuid, &username, entity_id, packet,
                            &game_tx, &instant_bus, &broadcast_tx,
                            &player_registry, &world, &command_args,
                            &mut conn, &mut last_keep_alive_id, &last_keep_alive_sent,
                        ).await?;
                    }
                    Err(basalt_net::Error::Protocol(
                        basalt_protocol::Error::UnknownPacket { id, .. }
                    )) => {
                        log::trace!(target: "basalt::net_task", "[{addr}] {username} unknown packet 0x{id:02x}");
                    }
                    Err(e) => {
                        log::info!(target: "basalt::net_task", "[{addr}] {username} disconnected: {e}");
                        break;
                    }
                }
            }

            // Branch 3: Relay game events from game loop
            output = output_rx.recv() => {
                match output {
                    Some(msg) => {
                        play_sender::write_server_output(&mut conn, &msg).await?;
                    }
                    None => {
                        log::debug!(target: "basalt::net_task", "[{addr}] {username} output channel closed");
                        break;
                    }
                }
            }

            // Branch 4: Receive instant broadcasts (chat)
            result = broadcast_rx.recv() => {
                match result {
                    Ok(msg) => {
                        play_sender::write_server_output(&mut conn, &msg).await?;
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        log::warn!(target: "basalt::net_task", "[{addr}] {username} missed {n} broadcast messages");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    Ok(())
}
