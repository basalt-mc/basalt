//! Per-player net task — TCP I/O, instant events, and game loop forwarding.
//!
//! Each connected player has one net task running as a tokio task.
//! The net task:
//! 1. Reads packets from the TCP connection
//! 2. Handles instant events (chat, commands) via `Arc<EventBus>` dispatch
//! 3. Forwards game-relevant packets (movement, blocks, inventory) to the game loop
//! 4. Relays output packets from the game loop + broadcast channel to TCP
//! 5. Handles keep-alive and tab-complete inline

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use basalt_api::context::{Response, ServerContext};
use basalt_api::events::{ChatMessageEvent, CommandEvent};
use basalt_events::EventBus;
use basalt_net::connection::{Connection, Play};
use basalt_protocol::packets::play::ServerboundPlayPacket;
use basalt_protocol::packets::play::chat::{
    ClientboundPlaySystemChat, ClientboundPlayTabComplete, ClientboundPlayTabCompleteMatches,
    ServerboundPlayTabComplete,
};
use basalt_protocol::packets::play::misc::ClientboundPlayKeepAlive;
use basalt_types::{Encode, EncodedSize, Uuid};
use dashmap::DashMap;
use tokio::sync::{broadcast, mpsc};

use crate::messages::{GameInput, ServerOutput};
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
    /// Command metadata for tab-complete and /help.
    pub command_args: Vec<CommandMeta>,
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
    let command_args = config.command_args;

    let mut broadcast_rx = broadcast_tx.subscribe();
    let mut keep_alive = tokio::time::interval(std::time::Duration::from_secs(15));
    keep_alive.tick().await;

    let mut last_keep_alive_id: i64 = 0;
    let mut last_keep_alive_sent = Instant::now();

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
                        if let ServerboundPlayPacket::TabComplete(tc) = &packet {
                            handle_tab_complete(&mut conn, &command_args, tc).await?;
                            continue;
                        }
                        handle_packet(
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

            // Branch 3: Relay output from game loop
            output = output_rx.recv() => {
                match output {
                    Some(ServerOutput::SendPacket { id, data }) => {
                        conn.write_packet_typed(id, &crate::helpers::RawPayload(data)).await?;
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
                    Ok(ServerOutput::SendPacket { id, data }) => {
                        conn.write_packet_typed(id, &crate::helpers::RawPayload(data)).await?;
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

/// Handles a single serverbound packet — instant or forwarded.
#[allow(clippy::too_many_arguments)]
async fn handle_packet(
    addr: SocketAddr,
    uuid: Uuid,
    username: &str,
    entity_id: i32,
    packet: ServerboundPlayPacket,
    game_tx: &mpsc::UnboundedSender<GameInput>,
    instant_bus: &EventBus,
    broadcast_tx: &broadcast::Sender<ServerOutput>,
    player_registry: &DashMap<Uuid, mpsc::Sender<ServerOutput>>,
    world: &Arc<basalt_world::World>,
    command_args: &[CommandMeta],
    conn: &mut Connection<Play>,
    last_keep_alive_id: &mut i64,
    last_keep_alive_sent: &Instant,
) -> crate::error::Result<()> {
    match packet {
        // -- Keep-alive: inline --
        ServerboundPlayPacket::KeepAlive(ka) => {
            if ka.keep_alive_id == *last_keep_alive_id {
                let rtt = last_keep_alive_sent.elapsed();
                log::trace!(target: "basalt::net_task", "[{addr}] {username} keep-alive OK (RTT: {}ms)", rtt.as_millis());
            } else {
                log::warn!(target: "basalt::net_task", "[{addr}] {username} keep-alive mismatch: expected {}, got {}", last_keep_alive_id, ka.keep_alive_id);
            }
        }

        // -- Instant: chat --
        ServerboundPlayPacket::ChatMessage(msg) => {
            if msg.message.len() > 256 {
                return Ok(());
            }
            log::info!(target: "basalt::net_task", "[{addr}] <{username}> {}", msg.message);
            let ctx = ServerContext::new(
                Arc::clone(world),
                uuid,
                entity_id,
                username.to_string(),
                0.0,
                0.0,
            );
            let mut event = ChatMessageEvent {
                username: username.to_string(),
                message: msg.message,
                cancelled: false,
            };
            instant_bus.dispatch(&mut event, &ctx);
            process_instant_responses(
                &ctx.drain_responses(),
                broadcast_tx,
                player_registry,
                uuid,
                conn,
            )
            .await?;
        }

        // -- Instant: commands --
        ServerboundPlayPacket::ChatCommand(cmd) => {
            log::info!(target: "basalt::net_task", "[{addr}] {username} issued /{}", cmd.command);
            let ctx = ServerContext::new(
                Arc::clone(world),
                uuid,
                entity_id,
                username.to_string(),
                0.0,
                0.0,
            );
            ctx.set_command_list(
                command_args
                    .iter()
                    .map(|c| (c.name.clone(), c.description.clone()))
                    .collect(),
            );
            let mut event = CommandEvent {
                command: cmd.command,
                player_uuid: uuid,
                cancelled: false,
            };
            instant_bus.dispatch(&mut event, &ctx);
            process_instant_responses(
                &ctx.drain_responses(),
                broadcast_tx,
                player_registry,
                uuid,
                conn,
            )
            .await?;
        }

        // -- Game loop: movement --
        ServerboundPlayPacket::Position(p) => {
            if is_valid_position(p.x, p.y, p.z) {
                let _ = game_tx.send(GameInput::Position {
                    uuid,
                    x: p.x,
                    y: p.y,
                    z: p.z,
                    on_ground: p.flags & 1 != 0,
                });
            }
        }
        ServerboundPlayPacket::PositionLook(p) => {
            if is_valid_position(p.x, p.y, p.z) {
                let _ = game_tx.send(GameInput::PositionLook {
                    uuid,
                    x: p.x,
                    y: p.y,
                    z: p.z,
                    yaw: p.yaw,
                    pitch: p.pitch,
                    on_ground: p.flags & 1 != 0,
                });
            }
        }
        ServerboundPlayPacket::Look(p) => {
            let _ = game_tx.send(GameInput::Look {
                uuid,
                yaw: p.yaw,
                pitch: p.pitch,
                on_ground: p.flags & 1 != 0,
            });
        }

        // -- Game loop: blocks --
        ServerboundPlayPacket::BlockDig(dig) => {
            let pos = dig.location;
            let _ = game_tx.send(GameInput::BlockDig {
                uuid,
                status: dig.status,
                x: pos.x,
                y: pos.y,
                z: pos.z,
                sequence: dig.sequence,
            });
        }
        ServerboundPlayPacket::BlockPlace(place) => {
            let _ = game_tx.send(GameInput::BlockPlace {
                uuid,
                x: place.location.x,
                y: place.location.y,
                z: place.location.z,
                direction: place.direction,
                sequence: place.sequence,
            });
        }

        // -- Game loop: inventory --
        ServerboundPlayPacket::HeldItemSlot(slot) => {
            let _ = game_tx.send(GameInput::HeldItemSlot {
                uuid,
                slot: slot.slot_id,
            });
        }
        ServerboundPlayPacket::SetCreativeSlot(creative) => {
            let _ = game_tx.send(GameInput::SetCreativeSlot {
                uuid,
                slot: creative.slot,
                item: creative.item,
            });
        }

        // -- Inline (no routing) --
        ServerboundPlayPacket::TeleportConfirm(_)
        | ServerboundPlayPacket::Flying(_)
        | ServerboundPlayPacket::PlayerLoaded(_) => {}

        // -- Ignored --
        ServerboundPlayPacket::CustomPayload(_)
        | ServerboundPlayPacket::PlayerInput(_)
        | ServerboundPlayPacket::TickEnd(_)
        | ServerboundPlayPacket::ChunkBatchReceived(_)
        | ServerboundPlayPacket::Pong(_)
        | ServerboundPlayPacket::MessageAcknowledgement(_)
        | ServerboundPlayPacket::ConfigurationAcknowledged(_)
        | ServerboundPlayPacket::UseItem(_)
        | ServerboundPlayPacket::ArmAnimation(_) => {}

        other => {
            log::trace!(target: "basalt::net_task", "[{addr}] {username} unhandled: {:?}", std::mem::discriminant(&other));
        }
    }
    Ok(())
}

/// Processes responses from instant event handlers.
///
/// Broadcasts go to the broadcast channel (all players). Targeted
/// messages (SendSystemChat, etc.) go directly to the player's TCP.
async fn process_instant_responses(
    responses: &[Response],
    broadcast_tx: &broadcast::Sender<ServerOutput>,
    _player_registry: &DashMap<Uuid, mpsc::Sender<ServerOutput>>,
    _source_uuid: Uuid,
    conn: &mut Connection<Play>,
) -> crate::error::Result<()> {
    for response in responses {
        match response {
            Response::Broadcast(basalt_api::BroadcastMessage::Chat { content }) => {
                let data = encode_packet(
                    ClientboundPlaySystemChat::PACKET_ID,
                    &ClientboundPlaySystemChat {
                        content: content.clone(),
                        is_action_bar: false,
                    },
                );
                let _ = broadcast_tx.send(data);
            }
            Response::SendSystemChat {
                content,
                action_bar,
            } => {
                let packet = ClientboundPlaySystemChat {
                    content: content.clone(),
                    is_action_bar: *action_bar,
                };
                conn.write_packet_typed(ClientboundPlaySystemChat::PACKET_ID, &packet)
                    .await?;
            }
            Response::SendPosition {
                teleport_id,
                x,
                y,
                z,
                yaw,
                pitch,
            } => {
                use basalt_protocol::packets::play::player::ClientboundPlayPosition;
                let packet = ClientboundPlayPosition {
                    teleport_id: *teleport_id,
                    x: *x,
                    y: *y,
                    z: *z,
                    dx: 0.0,
                    dy: 0.0,
                    dz: 0.0,
                    yaw: *yaw,
                    pitch: *pitch,
                    flags: 0,
                };
                conn.write_packet_typed(ClientboundPlayPosition::PACKET_ID, &packet)
                    .await?;
            }
            Response::SendGameStateChange { reason, value } => {
                use basalt_protocol::packets::play::player::ClientboundPlayGameStateChange;
                let packet = ClientboundPlayGameStateChange {
                    reason: *reason,
                    game_mode: *value,
                };
                conn.write_packet_typed(ClientboundPlayGameStateChange::PACKET_ID, &packet)
                    .await?;
            }
            // Game-loop concerns — not handled in instant context
            Response::Broadcast(_)
            | Response::SendBlockAck { .. }
            | Response::StreamChunks { .. }
            | Response::PersistChunk { .. } => {}
        }
    }
    Ok(())
}

/// Encodes a packet struct into a [`ServerOutput::SendPacket`].
fn encode_packet<P: Encode + EncodedSize>(packet_id: i32, packet: &P) -> ServerOutput {
    let mut data = Vec::with_capacity(packet.encoded_size());
    packet.encode(&mut data).expect("packet encoding failed");
    ServerOutput::SendPacket {
        id: packet_id,
        data,
    }
}

/// Handles a TabComplete request inline.
async fn handle_tab_complete(
    conn: &mut Connection<Play>,
    command_args: &[CommandMeta],
    tc: &ServerboundPlayTabComplete,
) -> crate::error::Result<()> {
    use basalt_command::Arg;

    let text = tc.text.trim_start_matches('/');
    let parts: Vec<&str> = text.split_whitespace().collect();

    let cmd_name = parts.first().copied().unwrap_or("");
    let arg_index = if text.ends_with(' ') {
        parts.len().saturating_sub(1)
    } else {
        parts.len().saturating_sub(2)
    };
    let prefix = if text.ends_with(' ') {
        ""
    } else {
        parts.last().copied().unwrap_or("")
    };

    let mut suggestions = Vec::new();

    if let Some(meta) = command_args.iter().find(|c| c.name == cmd_name) {
        let arg_lists: Vec<&Vec<basalt_command::CommandArg>> = if !meta.variants.is_empty() {
            meta.variants.iter().collect()
        } else {
            vec![&meta.args]
        };

        for arg_list in &arg_lists {
            if let Some(arg_def) = arg_list.get(arg_index) {
                match &arg_def.arg_type {
                    Arg::Options(choices) => {
                        for choice in choices {
                            if choice.starts_with(prefix) {
                                suggestions.push(ClientboundPlayTabCompleteMatches {
                                    r#match: choice.clone(),
                                    tooltip: None,
                                });
                            }
                        }
                    }
                    Arg::Boolean => {
                        for val in &["true", "false"] {
                            if val.starts_with(prefix) {
                                suggestions.push(ClientboundPlayTabCompleteMatches {
                                    r#match: (*val).to_string(),
                                    tooltip: None,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if !suggestions.is_empty() {
        let start = (tc.text.len() - prefix.len()) as i32;
        let response = ClientboundPlayTabComplete {
            transaction_id: tc.transaction_id,
            start,
            length: prefix.len() as i32,
            matches: suggestions,
        };
        conn.write_packet_typed(ClientboundPlayTabComplete::PACKET_ID, &response)
            .await?;
    }

    Ok(())
}

/// Maximum valid coordinate magnitude in Minecraft.
const MAX_COORDINATE: f64 = 30_000_000.0;

/// Validates that coordinates are finite and within the Minecraft world bounds.
fn is_valid_position(x: f64, y: f64, z: f64) -> bool {
    x.is_finite()
        && y.is_finite()
        && z.is_finite()
        && x.abs() <= MAX_COORDINATE
        && z.abs() <= MAX_COORDINATE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_valid_position_checks() {
        assert!(is_valid_position(0.0, 64.0, 0.0));
        assert!(is_valid_position(-30_000_000.0, 0.0, 30_000_000.0));
        assert!(!is_valid_position(f64::NAN, 0.0, 0.0));
        assert!(!is_valid_position(0.0, f64::INFINITY, 0.0));
        assert!(!is_valid_position(30_000_001.0, 0.0, 0.0));
    }
}
