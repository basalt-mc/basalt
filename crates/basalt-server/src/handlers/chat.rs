//! Chat and command handler plugin.
//!
//! Handles chat messages (broadcast to all players) and slash commands
//! (/say, /tp, /gamemode, /help). Command logic is ported from the
//! original `chat.rs` module, adapted to use the response queue
//! instead of direct connection writes.

use basalt_events::{EventBus, Stage};
use basalt_types::{NamedColor, TextColor, TextComponent};

use crate::context::{EventContext, Response};
use crate::events::{ChatMessageEvent, CommandEvent};
use crate::state::BroadcastMessage;

/// Handles chat messages and slash commands.
///
/// - **Process CommandEvent**: parses and executes commands, queuing responses
/// - **Post ChatMessageEvent**: broadcasts formatted chat to all players
pub struct ChatHandler;

impl ChatHandler {
    /// Registers chat and command handlers on the event bus.
    pub fn register(bus: &mut EventBus) {
        // Process: execute commands
        bus.on::<CommandEvent, EventContext>(Stage::Process, 0, |event, ctx| {
            handle_command(&event.command, ctx);
        });

        // Post: broadcast chat messages
        bus.on::<ChatMessageEvent, EventContext>(Stage::Post, 0, |event, ctx| {
            let component = crate::chat::build_chat_component(&event.username, &event.message);
            ctx.responses
                .push(Response::Broadcast(BroadcastMessage::Chat {
                    content: component.to_nbt(),
                }));
        });
    }
}

/// Parses and executes a slash command, queuing responses.
fn handle_command(command: &str, ctx: &EventContext) {
    let parts: Vec<&str> = command.splitn(2, ' ').collect();
    let cmd = parts[0];
    let args = parts.get(1).copied().unwrap_or("");

    match cmd {
        "say" => cmd_say(args, ctx),
        "tp" => cmd_tp(args, ctx),
        "gamemode" => cmd_gamemode(args, ctx),
        "help" => cmd_help(ctx),
        _ => {
            let msg = TextComponent::text(format!("Unknown command: /{cmd}"))
                .color(TextColor::Named(NamedColor::Red));
            ctx.responses.push(Response::SendSystemChat {
                content: msg.to_nbt(),
                action_bar: false,
            });
        }
    }
}

/// `/say <message>` — broadcasts a server message.
fn cmd_say(message: &str, ctx: &EventContext) {
    let msg = TextComponent::text("[Server] ")
        .color(TextColor::Named(NamedColor::LightPurple))
        .bold(true)
        .append(TextComponent::text(message).color(TextColor::Named(NamedColor::White)));
    ctx.responses.push(Response::SendSystemChat {
        content: msg.to_nbt(),
        action_bar: false,
    });
}

/// `/tp <x> <y> <z>` — teleports the player to the given coordinates.
fn cmd_tp(args: &str, ctx: &EventContext) {
    let coords: Vec<&str> = args.split_whitespace().collect();
    if coords.len() != 3 {
        let msg =
            TextComponent::text("Usage: /tp <x> <y> <z>").color(TextColor::Named(NamedColor::Red));
        ctx.responses.push(Response::SendSystemChat {
            content: msg.to_nbt(),
            action_bar: false,
        });
        return;
    }

    let Ok(x) = coords[0].parse::<f64>() else {
        send_error(ctx, "Invalid x coordinate");
        return;
    };
    let Ok(y) = coords[1].parse::<f64>() else {
        send_error(ctx, "Invalid y coordinate");
        return;
    };
    let Ok(z) = coords[2].parse::<f64>() else {
        send_error(ctx, "Invalid z coordinate");
        return;
    };

    ctx.responses.push(Response::SendPosition {
        teleport_id: 2,
        x,
        y,
        z,
        yaw: 0.0,
        pitch: 0.0,
    });

    let msg = TextComponent::text(format!("Teleported to {x}, {y}, {z}"))
        .color(TextColor::Named(NamedColor::Green));
    ctx.responses.push(Response::SendSystemChat {
        content: msg.to_nbt(),
        action_bar: false,
    });
}

/// `/gamemode <mode>` — changes the player's gamemode.
fn cmd_gamemode(args: &str, ctx: &EventContext) {
    let mode: f32 = match args.trim() {
        "survival" | "0" => 0.0,
        "creative" | "1" => 1.0,
        "adventure" | "2" => 2.0,
        "spectator" | "3" => 3.0,
        _ => {
            let msg =
                TextComponent::text("Usage: /gamemode <survival|creative|adventure|spectator>")
                    .color(TextColor::Named(NamedColor::Red));
            ctx.responses.push(Response::SendSystemChat {
                content: msg.to_nbt(),
                action_bar: false,
            });
            return;
        }
    };

    ctx.responses.push(Response::SendGameStateChange {
        reason: 3,
        value: mode,
    });

    let name = match mode as u8 {
        0 => "Survival",
        1 => "Creative",
        2 => "Adventure",
        _ => "Spectator",
    };
    let msg = TextComponent::text(format!("Game mode set to {name}"))
        .color(TextColor::Named(NamedColor::Green));
    ctx.responses.push(Response::SendSystemChat {
        content: msg.to_nbt(),
        action_bar: false,
    });
}

/// `/help` — shows available commands.
fn cmd_help(ctx: &EventContext) {
    let msg = TextComponent::text("Available commands:")
        .color(TextColor::Named(NamedColor::Gold))
        .append(
            TextComponent::text("\n /say <message>").color(TextColor::Named(NamedColor::Yellow)),
        )
        .append(
            TextComponent::text(" — broadcast a server message")
                .color(TextColor::Named(NamedColor::Gray)),
        )
        .append(
            TextComponent::text("\n /tp <x> <y> <z>").color(TextColor::Named(NamedColor::Yellow)),
        )
        .append(
            TextComponent::text(" — teleport to coordinates")
                .color(TextColor::Named(NamedColor::Gray)),
        )
        .append(
            TextComponent::text("\n /gamemode <mode>").color(TextColor::Named(NamedColor::Yellow)),
        )
        .append(
            TextComponent::text(" — change game mode").color(TextColor::Named(NamedColor::Gray)),
        )
        .append(TextComponent::text("\n /help").color(TextColor::Named(NamedColor::Yellow)))
        .append(TextComponent::text(" — show this help").color(TextColor::Named(NamedColor::Gray)));
    ctx.responses.push(Response::SendSystemChat {
        content: msg.to_nbt(),
        action_bar: false,
    });
}

/// Sends a red error message to the player.
fn send_error(ctx: &EventContext, message: &str) {
    let msg = TextComponent::text(message).color(TextColor::Named(NamedColor::Red));
    ctx.responses.push(Response::SendSystemChat {
        content: msg.to_nbt(),
        action_bar: false,
    });
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use basalt_events::Event;
    use basalt_types::Uuid;

    use super::*;
    use crate::state::ServerState;

    #[test]
    fn chat_message_broadcasts() {
        let state = ServerState::new();
        let ctx = EventContext::new(Arc::clone(&state));
        let mut event = ChatMessageEvent {
            username: "Steve".into(),
            message: "hello".into(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        ChatHandler::register(&mut bus);
        bus.dispatch(&mut event, &ctx);

        let responses = ctx.responses.drain();
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::Broadcast(BroadcastMessage::Chat { .. })
        ));
    }

    #[test]
    fn command_help_sends_system_chat() {
        let state = ServerState::new();
        let ctx = EventContext::new(Arc::clone(&state));
        let mut event = CommandEvent {
            command: "help".into(),
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        ChatHandler::register(&mut bus);
        bus.dispatch(&mut event, &ctx);

        let responses = ctx.responses.drain();
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::SendSystemChat {
                action_bar: false,
                ..
            }
        ));
    }

    #[test]
    fn command_tp_valid() {
        let state = ServerState::new();
        let ctx = EventContext::new(Arc::clone(&state));
        let mut event = CommandEvent {
            command: "tp 10 64 -5".into(),
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        ChatHandler::register(&mut bus);
        bus.dispatch(&mut event, &ctx);

        let responses = ctx.responses.drain();
        // SendPosition + SendSystemChat (confirmation)
        assert_eq!(responses.len(), 2);
        assert!(matches!(
            responses[0],
            Response::SendPosition {
                x,
                y,
                z,
                ..
            } if x == 10.0 && y == 64.0 && z == -5.0
        ));
    }

    #[test]
    fn command_tp_invalid_args() {
        let state = ServerState::new();
        let ctx = EventContext::new(Arc::clone(&state));
        let mut event = CommandEvent {
            command: "tp".into(),
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        ChatHandler::register(&mut bus);
        bus.dispatch(&mut event, &ctx);

        let responses = ctx.responses.drain();
        assert_eq!(responses.len(), 1);
        assert!(matches!(responses[0], Response::SendSystemChat { .. }));
    }

    #[test]
    fn command_gamemode_creative() {
        let state = ServerState::new();
        let ctx = EventContext::new(Arc::clone(&state));
        let mut event = CommandEvent {
            command: "gamemode creative".into(),
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        ChatHandler::register(&mut bus);
        bus.dispatch(&mut event, &ctx);

        let responses = ctx.responses.drain();
        // SendGameStateChange + SendSystemChat
        assert_eq!(responses.len(), 2);
        assert!(matches!(
            responses[0],
            Response::SendGameStateChange {
                reason: 3,
                value,
            } if value == 1.0
        ));
    }

    #[test]
    fn command_unknown() {
        let state = ServerState::new();
        let ctx = EventContext::new(Arc::clone(&state));
        let mut event = CommandEvent {
            command: "foobar".into(),
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        ChatHandler::register(&mut bus);
        bus.dispatch(&mut event, &ctx);

        let responses = ctx.responses.drain();
        assert_eq!(responses.len(), 1);
        assert!(matches!(responses[0], Response::SendSystemChat { .. }));
    }

    #[test]
    fn cancelled_chat_not_broadcast() {
        let state = ServerState::new();
        let ctx = EventContext::new(Arc::clone(&state));
        let mut event = ChatMessageEvent {
            username: "Steve".into(),
            message: "spam".into(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        bus.on::<ChatMessageEvent, EventContext>(Stage::Validate, 0, |event, _| {
            event.cancel();
        });
        ChatHandler::register(&mut bus);
        bus.dispatch(&mut event, &ctx);

        assert!(ctx.responses.drain().is_empty());
    }

    #[test]
    fn command_say() {
        let state = ServerState::new();
        let ctx = EventContext::new(Arc::clone(&state));
        let mut event = CommandEvent {
            command: "say hello world".into(),
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        ChatHandler::register(&mut bus);
        bus.dispatch(&mut event, &ctx);

        let responses = ctx.responses.drain();
        assert_eq!(responses.len(), 1);
        assert!(matches!(responses[0], Response::SendSystemChat { .. }));
    }
}
