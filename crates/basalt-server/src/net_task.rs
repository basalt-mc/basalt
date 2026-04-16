//! Per-player net task — TCP I/O, packet fan-out, and output relay.
//!
//! Each connected player has one net task running as a tokio task.
//! The net task:
//! 1. Reads packets from the TCP connection
//! 2. Classifies each packet and fans out to the network and/or game channels
//! 3. Relays output packets from the loops back to the TCP connection
//! 4. Handles keep-alive probes inline (not routed to any loop)
//!
//! The net task is the only code that touches the TCP socket. The loops
//! communicate with it exclusively through MPSC channels.

use std::net::SocketAddr;
use std::time::Instant;

use basalt_net::connection::{Connection, Play};
use basalt_protocol::packets::play::ServerboundPlayPacket;
use basalt_protocol::packets::play::chat::{
    ClientboundPlayTabComplete, ClientboundPlayTabCompleteMatches, ServerboundPlayTabComplete,
};
use basalt_protocol::packets::play::misc::ClientboundPlayKeepAlive;
use basalt_types::Uuid;
use tokio::sync::mpsc;

use crate::messages::{GameInput, NetworkInput, ServerOutput};
use crate::state::CommandMeta;

/// Runs the per-player net task until disconnect.
///
/// Per-player net task configuration.
pub(crate) struct NetTaskConfig {
    /// Player UUID.
    pub uuid: Uuid,
    /// Player display name.
    pub username: String,
    /// Sender for the network loop.
    pub network_tx: mpsc::UnboundedSender<NetworkInput>,
    /// Sender for the game loop.
    pub game_tx: mpsc::UnboundedSender<GameInput>,
}

/// This function is spawned as a tokio task for each player that
/// reaches the Play state. It handles TCP I/O, packet classification
/// (fan-out), keep-alive supervision, and output relay.
pub(crate) async fn run_net_task(
    mut conn: Connection<Play>,
    addr: SocketAddr,
    config: NetTaskConfig,
    mut output_rx: mpsc::Receiver<ServerOutput>,
    command_args: &[CommandMeta],
) -> crate::error::Result<()> {
    let uuid = config.uuid;
    let username = config.username;
    let network_tx = config.network_tx;
    let game_tx = config.game_tx;
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

            // Branch 2: Read packets from TCP and fan out
            result = conn.read_packet() => {
                match result {
                    Ok(packet) => {
                        // TabComplete is handled inline (not routed)
                        if let ServerboundPlayPacket::TabComplete(tc) = &packet {
                            handle_tab_complete(&mut conn, command_args, tc).await?;
                            continue;
                        }
                        fan_out(
                            addr,
                            uuid,
                            &username,
                            packet,
                            &network_tx,
                            &game_tx,
                            &mut last_keep_alive_id,
                            &last_keep_alive_sent,
                        );
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

            // Branch 3: Relay output from loops to TCP
            output = output_rx.recv() => {
                match output {
                    Some(ServerOutput::SendPacket { id, data }) => {
                        conn.write_packet_typed(id, &crate::helpers::RawPayload(data)).await?;
                    }
                    None => {
                        // Both loops dropped their senders — server shutting down
                        log::debug!(target: "basalt::net_task", "[{addr}] {username} output channel closed");
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Classifies a serverbound packet and sends it to the correct channel(s).
///
/// **Fan-out table:**
///
/// | Packet | Destination |
/// |--------|------------|
/// | Position, Look, PositionLook | network_tx |
/// | ChatMessage, ChatCommand | network_tx |
/// | BlockDig, BlockPlace | network_tx AND game_tx |
/// | HeldItemSlot, SetCreativeSlot | game_tx |
/// | TeleportConfirm | game_tx |
/// | KeepAlive | inline (not routed) |
/// | Flying, CustomPayload, etc. | dropped |
#[allow(clippy::too_many_arguments)]
fn fan_out(
    addr: SocketAddr,
    uuid: Uuid,
    username: &str,
    packet: ServerboundPlayPacket,
    network_tx: &mpsc::UnboundedSender<NetworkInput>,
    game_tx: &mpsc::UnboundedSender<GameInput>,
    last_keep_alive_id: &mut i64,
    last_keep_alive_sent: &Instant,
) {
    match packet {
        // -- Keep-alive: handled inline --
        ServerboundPlayPacket::KeepAlive(ka) => {
            if ka.keep_alive_id == *last_keep_alive_id {
                let rtt = last_keep_alive_sent.elapsed();
                log::trace!(target: "basalt::net_task", "[{addr}] {username} keep-alive OK (RTT: {}ms)", rtt.as_millis());
            } else {
                log::warn!(target: "basalt::net_task", "[{addr}] {username} keep-alive mismatch: expected {}, got {}", last_keep_alive_id, ka.keep_alive_id);
            }
        }

        // -- Network loop only: movement --
        ServerboundPlayPacket::Position(p) => {
            if is_valid_position(p.x, p.y, p.z) {
                let _ = network_tx.send(NetworkInput::Position {
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
                let _ = network_tx.send(NetworkInput::PositionLook {
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
            let _ = network_tx.send(NetworkInput::Look {
                uuid,
                yaw: p.yaw,
                pitch: p.pitch,
                on_ground: p.flags & 1 != 0,
            });
        }

        // -- Network loop only: chat --
        ServerboundPlayPacket::ChatMessage(msg) => {
            let _ = network_tx.send(NetworkInput::ChatMessage {
                uuid,
                username: username.to_string(),
                message: msg.message,
            });
        }
        ServerboundPlayPacket::ChatCommand(cmd) => {
            let _ = network_tx.send(NetworkInput::ChatCommand {
                uuid,
                command: cmd.command,
            });
        }

        // -- Game loop only: blocks --
        // No fan-out to network loop — the Minecraft client needs the ack
        // and BlockChanged to arrive in the same tick. Splitting them across
        // loops causes flicker. The game loop handles ack + broadcast together.
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

        // -- Game loop only: inventory --
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

        // -- Inline state updates (no routing) --
        ServerboundPlayPacket::TeleportConfirm(_)
        | ServerboundPlayPacket::Flying(_)
        | ServerboundPlayPacket::PlayerLoaded(_) => {}

        // -- Ignored packets --
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
    use basalt_protocol::packets::play::misc::ServerboundPlayKeepAlive;
    use basalt_protocol::packets::play::player::{
        ServerboundPlayLook, ServerboundPlayPosition, ServerboundPlayPositionLook,
    };
    use basalt_protocol::packets::play::world::{
        ServerboundPlayBlockDig, ServerboundPlayBlockPlace,
    };
    use std::time::Instant;

    fn test_channels() -> (
        mpsc::UnboundedSender<NetworkInput>,
        mpsc::UnboundedReceiver<NetworkInput>,
        mpsc::UnboundedSender<GameInput>,
        mpsc::UnboundedReceiver<GameInput>,
    ) {
        let (ntx, nrx) = mpsc::unbounded_channel();
        let (gtx, grx) = mpsc::unbounded_channel();
        (ntx, nrx, gtx, grx)
    }

    fn test_addr() -> SocketAddr {
        "127.0.0.1:12345".parse().unwrap()
    }

    #[test]
    fn fan_out_position_goes_to_network() {
        let (ntx, mut nrx, gtx, mut grx) = test_channels();
        let mut ka_id = 0i64;
        let ka_sent = Instant::now();
        let uuid = Uuid::default();

        fan_out(
            test_addr(),
            uuid,
            "Steve",
            ServerboundPlayPacket::Position(ServerboundPlayPosition {
                x: 1.0,
                y: 64.0,
                z: -3.0,
                flags: 1,
            }),
            &ntx,
            &gtx,
            &mut ka_id,
            &ka_sent,
        );

        assert!(nrx.try_recv().is_ok(), "position should go to network_tx");
        assert!(grx.try_recv().is_err(), "position should not go to game_tx");
    }

    #[test]
    fn fan_out_look_goes_to_network() {
        let (ntx, mut nrx, gtx, mut grx) = test_channels();
        let mut ka_id = 0i64;
        let ka_sent = Instant::now();

        fan_out(
            test_addr(),
            Uuid::default(),
            "Steve",
            ServerboundPlayPacket::Look(ServerboundPlayLook {
                yaw: 90.0,
                pitch: 0.0,
                flags: 0,
            }),
            &ntx,
            &gtx,
            &mut ka_id,
            &ka_sent,
        );

        assert!(nrx.try_recv().is_ok());
        assert!(grx.try_recv().is_err());
    }

    #[test]
    fn fan_out_position_look_goes_to_network() {
        let (ntx, mut nrx, gtx, mut grx) = test_channels();
        let mut ka_id = 0i64;
        let ka_sent = Instant::now();

        fan_out(
            test_addr(),
            Uuid::default(),
            "Steve",
            ServerboundPlayPacket::PositionLook(ServerboundPlayPositionLook {
                x: 1.0,
                y: 64.0,
                z: -3.0,
                yaw: 45.0,
                pitch: 10.0,
                flags: 0,
            }),
            &ntx,
            &gtx,
            &mut ka_id,
            &ka_sent,
        );

        assert!(nrx.try_recv().is_ok());
        assert!(grx.try_recv().is_err());
    }

    #[test]
    fn fan_out_chat_goes_to_network() {
        let (ntx, mut nrx, gtx, mut grx) = test_channels();
        let mut ka_id = 0i64;
        let ka_sent = Instant::now();

        fan_out(
            test_addr(),
            Uuid::default(),
            "Steve",
            ServerboundPlayPacket::ChatMessage(
                basalt_protocol::packets::play::chat::ServerboundPlayChatMessage {
                    message: "hello".into(),
                    timestamp: 0,
                    salt: 0,
                    signature: None,
                    offset: 0,
                    acknowledged: vec![],
                },
            ),
            &ntx,
            &gtx,
            &mut ka_id,
            &ka_sent,
        );

        assert!(nrx.try_recv().is_ok());
        assert!(grx.try_recv().is_err());
    }

    #[test]
    fn fan_out_block_dig_goes_to_game_only() {
        let (ntx, mut nrx, gtx, mut grx) = test_channels();
        let mut ka_id = 0i64;
        let ka_sent = Instant::now();

        fan_out(
            test_addr(),
            Uuid::default(),
            "Steve",
            ServerboundPlayPacket::BlockDig(ServerboundPlayBlockDig {
                status: 0,
                location: basalt_types::Position::new(5, 64, 3),
                face: 1,
                sequence: 42,
            }),
            &ntx,
            &gtx,
            &mut ka_id,
            &ka_sent,
        );

        assert!(
            nrx.try_recv().is_err(),
            "block dig should NOT go to network"
        );
        assert!(grx.try_recv().is_ok(), "block dig should go to game");
    }

    #[test]
    fn fan_out_block_place_goes_to_game_only() {
        let (ntx, mut nrx, gtx, mut grx) = test_channels();
        let mut ka_id = 0i64;
        let ka_sent = Instant::now();

        fan_out(
            test_addr(),
            Uuid::default(),
            "Steve",
            ServerboundPlayPacket::BlockPlace(ServerboundPlayBlockPlace {
                hand: 0,
                location: basalt_types::Position::new(5, 63, 3),
                direction: 1,
                cursor_x: 0.5,
                cursor_y: 1.0,
                cursor_z: 0.5,
                inside_block: false,
                world_border_hit: false,
                sequence: 10,
            }),
            &ntx,
            &gtx,
            &mut ka_id,
            &ka_sent,
        );

        assert!(
            nrx.try_recv().is_err(),
            "block place should NOT go to network"
        );
        assert!(grx.try_recv().is_ok(), "block place should go to game");
    }

    #[test]
    fn fan_out_keep_alive_handled_inline() {
        let (ntx, mut nrx, gtx, mut grx) = test_channels();
        let mut ka_id = 5i64;
        let ka_sent = Instant::now();

        fan_out(
            test_addr(),
            Uuid::default(),
            "Steve",
            ServerboundPlayPacket::KeepAlive(ServerboundPlayKeepAlive { keep_alive_id: 5 }),
            &ntx,
            &gtx,
            &mut ka_id,
            &ka_sent,
        );

        assert!(
            nrx.try_recv().is_err(),
            "keep-alive should not go to any channel"
        );
        assert!(
            grx.try_recv().is_err(),
            "keep-alive should not go to any channel"
        );
    }

    #[test]
    fn fan_out_invalid_position_dropped() {
        let (ntx, mut nrx, gtx, _grx) = test_channels();
        let mut ka_id = 0i64;
        let ka_sent = Instant::now();

        fan_out(
            test_addr(),
            Uuid::default(),
            "Steve",
            ServerboundPlayPacket::Position(ServerboundPlayPosition {
                x: f64::NAN,
                y: 64.0,
                z: 0.0,
                flags: 0,
            }),
            &ntx,
            &gtx,
            &mut ka_id,
            &ka_sent,
        );

        assert!(
            nrx.try_recv().is_err(),
            "invalid position should be dropped"
        );
    }

    #[test]
    fn is_valid_position_checks() {
        assert!(is_valid_position(0.0, 64.0, 0.0));
        assert!(is_valid_position(-30_000_000.0, 0.0, 30_000_000.0));
        assert!(!is_valid_position(f64::NAN, 0.0, 0.0));
        assert!(!is_valid_position(0.0, f64::INFINITY, 0.0));
        assert!(!is_valid_position(30_000_001.0, 0.0, 0.0));
    }
}
