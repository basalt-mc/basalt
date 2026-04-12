//! Chat and command plugin.
//!
//! Handles chat messages (broadcast to all players) and slash commands
//! (/say, /tp, /gamemode, /help). Uses only the public `ServerContext`
//! API — no internal server types.

use basalt_api::context::ServerContext;
use basalt_api::prelude::*;
use basalt_types::{NamedColor, TextColor, TextComponent};

/// Handles chat messages and slash commands.
///
/// - **Process CommandEvent**: parses and executes commands
/// - **Post ChatMessageEvent**: broadcasts formatted chat to all players
pub struct ChatPlugin;

impl Plugin for ChatPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "chat",
            version: "0.1.0",
            author: Some("Basalt"),
            dependencies: &[],
        }
    }

    fn on_enable(&self, registrar: &mut EventRegistrar) {
        registrar.on::<CommandEvent>(Stage::Process, 0, |event, ctx| {
            handle_command(&event.command, ctx);
        });

        registrar.on::<ChatMessageEvent>(Stage::Post, 0, |event, _ctx| {
            let component = build_chat_component(&event.username, &event.message);
            _ctx.broadcast_message_component(&component);
        });
    }
}

/// Builds a formatted chat text component for `<username> message`.
pub fn build_chat_component(username: &str, message: &str) -> TextComponent {
    TextComponent::text("<")
        .append(TextComponent::text(username).color(TextColor::Named(NamedColor::Aqua)))
        .append(TextComponent::text("> "))
        .append(TextComponent::text(message))
}

/// Parses and executes a slash command via ServerContext methods.
fn handle_command(command: &str, ctx: &ServerContext) {
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
            ctx.send_message_component(&msg);
        }
    }
}

/// `/say <message>` — broadcasts a server message.
fn cmd_say(message: &str, ctx: &ServerContext) {
    let msg = TextComponent::text("[Server] ")
        .color(TextColor::Named(NamedColor::LightPurple))
        .bold(true)
        .append(TextComponent::text(message).color(TextColor::Named(NamedColor::White)));
    ctx.send_message_component(&msg);
}

/// `/tp <x> <y> <z>` — teleports the player to the given coordinates.
fn cmd_tp(args: &str, ctx: &ServerContext) {
    let coords: Vec<&str> = args.split_whitespace().collect();
    if coords.len() != 3 {
        ctx.send_message_component(
            &TextComponent::text("Usage: /tp <x> <y> <z>").color(TextColor::Named(NamedColor::Red)),
        );
        return;
    }

    let Ok(x) = coords[0].parse::<f64>() else {
        ctx.send_message_component(
            &TextComponent::text("Invalid x coordinate").color(TextColor::Named(NamedColor::Red)),
        );
        return;
    };
    let Ok(y) = coords[1].parse::<f64>() else {
        ctx.send_message_component(
            &TextComponent::text("Invalid y coordinate").color(TextColor::Named(NamedColor::Red)),
        );
        return;
    };
    let Ok(z) = coords[2].parse::<f64>() else {
        ctx.send_message_component(
            &TextComponent::text("Invalid z coordinate").color(TextColor::Named(NamedColor::Red)),
        );
        return;
    };

    ctx.teleport(x, y, z, 0.0, 0.0);
    ctx.send_message_component(
        &TextComponent::text(format!("Teleported to {x}, {y}, {z}"))
            .color(TextColor::Named(NamedColor::Green)),
    );
}

/// `/gamemode <mode>` — changes the player's gamemode.
fn cmd_gamemode(args: &str, ctx: &ServerContext) {
    let mode: u8 = match args.trim() {
        "survival" | "0" => 0,
        "creative" | "1" => 1,
        "adventure" | "2" => 2,
        "spectator" | "3" => 3,
        _ => {
            ctx.send_message_component(
                &TextComponent::text("Usage: /gamemode <survival|creative|adventure|spectator>")
                    .color(TextColor::Named(NamedColor::Red)),
            );
            return;
        }
    };

    ctx.set_gamemode(mode);

    let name = match mode {
        0 => "Survival",
        1 => "Creative",
        2 => "Adventure",
        _ => "Spectator",
    };
    ctx.send_message_component(
        &TextComponent::text(format!("Game mode set to {name}"))
            .color(TextColor::Named(NamedColor::Green)),
    );
}

/// `/help` — shows available commands.
fn cmd_help(ctx: &ServerContext) {
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
    ctx.send_message_component(&msg);
}

#[cfg(test)]
mod tests {
    use basalt_api::{Event, EventBus, Response};
    use basalt_types::Uuid;

    use super::*;

    fn test_world() -> &'static basalt_world::World {
        use std::sync::OnceLock;
        static WORLD: OnceLock<basalt_world::World> = OnceLock::new();
        WORLD.get_or_init(|| basalt_world::World::new_memory(42))
    }

    fn test_ctx() -> ServerContext {
        ServerContext::new(test_world(), Uuid::default(), 1, "Steve".into())
    }

    #[test]
    fn chat_message_broadcasts() {
        let ctx = test_ctx();
        let mut event = ChatMessageEvent {
            username: "Steve".into(),
            message: "hello".into(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        let mut registrar = EventRegistrar::new(&mut bus);
        ChatPlugin.on_enable(&mut registrar);
        bus.dispatch(&mut event, &ctx);

        let responses = ctx.drain_responses();
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::Broadcast(BroadcastMessage::Chat { .. })
        ));
    }

    #[test]
    fn command_help() {
        let ctx = test_ctx();
        let mut event = CommandEvent {
            command: "help".into(),
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        let mut registrar = EventRegistrar::new(&mut bus);
        ChatPlugin.on_enable(&mut registrar);
        bus.dispatch(&mut event, &ctx);

        let responses = ctx.drain_responses();
        assert_eq!(responses.len(), 1);
        assert!(matches!(responses[0], Response::SendSystemChat { .. }));
    }

    #[test]
    fn command_tp_valid() {
        let ctx = test_ctx();
        let mut event = CommandEvent {
            command: "tp 10 64 -5".into(),
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        let mut registrar = EventRegistrar::new(&mut bus);
        ChatPlugin.on_enable(&mut registrar);
        bus.dispatch(&mut event, &ctx);

        let responses = ctx.drain_responses();
        assert_eq!(responses.len(), 2);
        assert!(matches!(responses[0], Response::SendPosition { .. }));
    }

    #[test]
    fn command_tp_invalid() {
        let ctx = test_ctx();
        let mut event = CommandEvent {
            command: "tp".into(),
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        let mut registrar = EventRegistrar::new(&mut bus);
        ChatPlugin.on_enable(&mut registrar);
        bus.dispatch(&mut event, &ctx);

        assert_eq!(ctx.drain_responses().len(), 1);
    }

    #[test]
    fn command_gamemode() {
        let ctx = test_ctx();
        let mut event = CommandEvent {
            command: "gamemode creative".into(),
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        let mut registrar = EventRegistrar::new(&mut bus);
        ChatPlugin.on_enable(&mut registrar);
        bus.dispatch(&mut event, &ctx);

        let responses = ctx.drain_responses();
        assert_eq!(responses.len(), 2);
        assert!(matches!(responses[0], Response::SendGameStateChange { .. }));
    }

    #[test]
    fn command_unknown() {
        let ctx = test_ctx();
        let mut event = CommandEvent {
            command: "foobar".into(),
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        let mut registrar = EventRegistrar::new(&mut bus);
        ChatPlugin.on_enable(&mut registrar);
        bus.dispatch(&mut event, &ctx);

        assert_eq!(ctx.drain_responses().len(), 1);
    }

    #[test]
    fn command_say() {
        let ctx = test_ctx();
        let mut event = CommandEvent {
            command: "say hello world".into(),
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        let mut registrar = EventRegistrar::new(&mut bus);
        ChatPlugin.on_enable(&mut registrar);
        bus.dispatch(&mut event, &ctx);

        assert_eq!(ctx.drain_responses().len(), 1);
    }

    #[test]
    fn cancelled_chat_not_broadcast() {
        let ctx = test_ctx();
        let mut event = ChatMessageEvent {
            username: "Steve".into(),
            message: "spam".into(),
            cancelled: false,
        };

        let mut bus = EventBus::new();
        let mut registrar = EventRegistrar::new(&mut bus);
        registrar.on::<ChatMessageEvent>(Stage::Validate, 0, |event, _| {
            event.cancel();
        });
        ChatPlugin.on_enable(&mut registrar);
        bus.dispatch(&mut event, &ctx);

        assert!(ctx.drain_responses().is_empty());
    }
}
