//! Serverbound packet handling — instant events, game loop forwarding, and tab-complete.
//!
//! [`handle_packet`] dispatches each serverbound Play packet to the right
//! destination: keep-alive inline, chat/commands via the instant event bus,
//! movement/blocks/inventory to the game loop channel.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use basalt_api::components::Rotation;
use basalt_api::context::Response;

use crate::context::ServerContext;
use basalt_api::events::EventBus;
use basalt_api::events::{ChatMessageEvent, CommandEvent, RawPacketEvent};
use basalt_api::player::PlayerInfo;
use basalt_mc_protocol::packets::play::ServerboundPlayPacket;
use basalt_mc_protocol::packets::play::chat::{
    ClientboundPlaySystemChat, ClientboundPlayTabComplete, ClientboundPlayTabCompleteMatches,
    ServerboundPlayTabComplete,
};
use basalt_mc_protocol::packets::play::player::{
    ClientboundPlayGameStateChange, ClientboundPlayPosition,
};
use basalt_net::connection::{Connection, Play};
use basalt_types::Uuid;
use dashmap::DashMap;
use tokio::sync::{broadcast, mpsc};

use crate::messages::{GameInput, ServerOutput};
use crate::state::CommandMeta;

/// Handles a single serverbound packet -- instant or forwarded.
#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_packet(
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
    // Pre-dispatch hook — fire `RawPacketEvent` so plugins (anti-cheat,
    // telemetry, packet logging) can inspect or cancel the packet
    // before any domain logic runs. Cancellation drops the packet.
    let raw_ctx = ServerContext::new(
        Arc::clone(world),
        PlayerInfo {
            uuid,
            entity_id,
            username: username.to_string(),
            rotation: Rotation {
                yaw: 0.0,
                pitch: 0.0,
            },
            position: basalt_api::components::Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        },
    );
    let mut raw_event = RawPacketEvent {
        packet: packet.clone(),
        cancelled: false,
    };
    if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        instant_bus.dispatch(
            &mut raw_event,
            &raw_ctx as &dyn basalt_api::context::Context,
        );
    })) {
        let msg = panic
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| panic.downcast_ref::<String>().map(|s| s.as_str()))
            .unwrap_or("unknown panic");
        log::error!(target: "basalt::net_task", "[{addr}] Plugin handler panicked on RawPacketEvent: {msg}");
    }
    if raw_event.cancelled {
        log::trace!(target: "basalt::net_task", "[{addr}] {username} packet cancelled by plugin");
        return Ok(());
    }

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
                PlayerInfo {
                    uuid,
                    entity_id,
                    username: username.to_string(),
                    rotation: Rotation {
                        yaw: 0.0,
                        pitch: 0.0,
                    },
                    // Net task lacks ECS access; instant events don't
                    // currently expose position to plugins.
                    position: basalt_api::components::Position {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                },
            );
            let mut event = ChatMessageEvent {
                message: msg.message,
                cancelled: false,
            };
            if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                instant_bus
                    .dispatch(&mut event, &ctx as &dyn basalt_api::context::Context);
            })) {
                let msg = panic
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| panic.downcast_ref::<String>().map(|s| s.as_str()))
                    .unwrap_or("unknown panic");
                log::error!(target: "basalt::net_task", "[{addr}] Plugin handler panicked on ChatMessage: {msg}");
            }
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
                PlayerInfo {
                    uuid,
                    entity_id,
                    username: username.to_string(),
                    rotation: Rotation {
                        yaw: 0.0,
                        pitch: 0.0,
                    },
                    // Net task lacks ECS access; instant events don't
                    // currently expose position to plugins.
                    position: basalt_api::components::Position {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                },
            );
            ctx.set_command_list(
                command_args
                    .iter()
                    .map(|c| (c.name.clone(), c.description.clone()))
                    .collect(),
            );
            let mut event = CommandEvent {
                command: cmd.command,
                cancelled: false,
            };
            if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                instant_bus
                    .dispatch(&mut event, &ctx as &dyn basalt_api::context::Context);
            })) {
                let msg = panic
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| panic.downcast_ref::<String>().map(|s| s.as_str()))
                    .unwrap_or("unknown panic");
                log::error!(target: "basalt::net_task", "[{addr}] Plugin handler panicked on Command: {msg}");
            }
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

        // -- Game loop: window click --
        ServerboundPlayPacket::WindowClick(click) => {
            let _ = game_tx.send(GameInput::WindowClick {
                uuid,
                slot: click.slot,
                button: click.mouse_button,
                mode: click.mode,
                changed_slots: click
                    .changed_slots
                    .into_iter()
                    .map(|cs| (cs.location, cs.item))
                    .collect(),
                cursor_item: click.cursor_item,
            });
        }

        // -- Game loop: close window --
        ServerboundPlayPacket::CloseWindow(_) => {
            let _ = game_tx.send(GameInput::CloseWindow { uuid });
        }

        // -- Game loop: entity action (sneak, sprint, etc.) --
        ServerboundPlayPacket::EntityAction(action) => {
            let _ = game_tx.send(GameInput::EntityAction {
                uuid,
                action_id: action.action_id,
            });
        }

        // -- Game loop: place recipe (ghost preview + auto-fill) --
        ServerboundPlayPacket::CraftRecipeRequest(req) => {
            let _ = game_tx.send(GameInput::PlaceRecipe {
                uuid,
                window_id: req.window_id,
                display_id: req.recipe_id,
                make_all: req.make_all,
            });
        }

        // -- Inline (no routing) --
        ServerboundPlayPacket::TeleportConfirm(_)
        | ServerboundPlayPacket::Flying(_)
        | ServerboundPlayPacket::PlayerLoaded(_) => {}

        // -- Forwarded: chunk batch rate ACK --
        ServerboundPlayPacket::ChunkBatchReceived(ack) => {
            let _ = game_tx.send(GameInput::ChunkBatchAck {
                uuid,
                chunks_per_tick: ack.chunks_per_tick,
            });
        }

        // -- Ignored --
        ServerboundPlayPacket::CustomPayload(_)
        | ServerboundPlayPacket::PlayerInput(_)
        | ServerboundPlayPacket::TickEnd(_)
        | ServerboundPlayPacket::Pong(_)
        | ServerboundPlayPacket::MessageAcknowledgement(_)
        | ServerboundPlayPacket::ConfigurationAcknowledged(_)
        | ServerboundPlayPacket::UseItem(_)
        | ServerboundPlayPacket::ArmAnimation(_)
        // Recipe book settings (which tabs are open / filtering) and
        // displayed-recipe notifications are accepted but ignored —
        // tracking the player's UI tab state isn't useful server-side.
        | ServerboundPlayPacket::RecipeBook(_)
        | ServerboundPlayPacket::DisplayedRecipe(_) => {}

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
            Response::Broadcast(basalt_api::broadcast::BroadcastMessage::Chat { content }) => {
                let bc = Arc::new(crate::messages::SharedBroadcast::single(
                    ClientboundPlaySystemChat::PACKET_ID,
                    ClientboundPlaySystemChat {
                        content: content.clone(),
                        is_action_bar: false,
                    },
                ));
                let _ = broadcast_tx.send(ServerOutput::Cached(bc));
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
                position,
                rotation,
            } => {
                let packet = ClientboundPlayPosition {
                    teleport_id: *teleport_id,
                    x: position.x,
                    y: position.y,
                    z: position.z,
                    dx: 0.0,
                    dy: 0.0,
                    dz: 0.0,
                    yaw: rotation.yaw,
                    pitch: rotation.pitch,
                    flags: 0,
                };
                conn.write_packet_typed(ClientboundPlayPosition::PACKET_ID, &packet)
                    .await?;
            }
            Response::SendGameStateChange { reason, value } => {
                let packet = ClientboundPlayGameStateChange {
                    reason: *reason,
                    game_mode: *value,
                };
                conn.write_packet_typed(ClientboundPlayGameStateChange::PACKET_ID, &packet)
                    .await?;
            }
            // Game-loop concerns -- not handled in instant context
            Response::Broadcast(_)
            | Response::SendBlockAck { .. }
            | Response::StreamChunks(_)
            | Response::PersistChunk(_)
            | Response::SpawnDroppedItem { .. }
            | Response::OpenChest(_)
            | Response::OpenCraftingTable { .. }
            | Response::OpenContainer(_)
            | Response::BroadcastBlockAction { .. }
            | Response::NotifyContainerViewers { .. }
            | Response::DestroyBlockEntity { .. }
            | Response::UnlockRecipe { .. }
            | Response::LockRecipe { .. } => {}
        }
    }
    Ok(())
}

/// Handles a TabComplete request inline.
pub(super) async fn handle_tab_complete(
    conn: &mut Connection<Play>,
    command_args: &[CommandMeta],
    tc: &ServerboundPlayTabComplete,
) -> crate::error::Result<()> {
    use basalt_api::command::Arg;

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
        let arg_lists: Vec<&Vec<basalt_api::command::CommandArg>> = if !meta.variants.is_empty() {
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
