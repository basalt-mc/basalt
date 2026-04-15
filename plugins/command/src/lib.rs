//! Command plugin for gameplay and administration commands.
//!
//! Registers all in-game commands via the `PluginRegistrar` builder
//! API. Commands use typed arguments with auto-validation and
//! tab-completion.

use basalt_api::prelude::*;
use basalt_types::{NamedColor, TextColor, TextComponent};

/// Gameplay and administration command plugin.
///
/// Registers: /tp, /gamemode, /say, /stop, /kick, /list, /help
pub struct CommandPlugin;

impl Plugin for CommandPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "command",
            version: "0.1.0",
            author: Some("Basalt"),
            dependencies: &[],
        }
    }

    fn on_enable(&self, registrar: &mut PluginRegistrar) {
        // /tp — 3 variants matching vanilla:
        //   /tp <targets> <destination>
        //   /tp <destination>
        //   /tp <location>
        registrar
            .command("tp")
            .description("Teleport to a player or coordinates")
            .variant(|v| v.arg("destination", Arg::Player))
            .variant(|v| v.arg("location", Arg::Vec3))
            .variant(|v| {
                v.arg("targets", Arg::Entity)
                    .arg("destination", Arg::Player)
            })
            .variant(|v| v.arg("targets", Arg::Entity).arg("location", Arg::Vec3))
            .handler(|args, ctx| {
                if let Some(location) = args.get_string("location") {
                    let coords: Vec<&str> = location.split_whitespace().collect();
                    if coords.len() == 3
                        && let (Ok(x), Ok(y), Ok(z)) = (
                            coords[0].parse::<f64>(),
                            coords[1].parse::<f64>(),
                            coords[2].parse::<f64>(),
                        )
                    {
                        ctx.teleport(x, y, z, 0.0, 0.0);
                        ctx.send_message_component(
                            &TextComponent::text(format!("Teleported to {x}, {y}, {z}"))
                                .color(TextColor::Named(NamedColor::Green)),
                        );
                        return;
                    }
                    ctx.send_message_component(
                        &TextComponent::text("Invalid coordinates")
                            .color(TextColor::Named(NamedColor::Red)),
                    );
                } else if let Some(target) = args.get_string("destination") {
                    ctx.send_message_component(
                        &TextComponent::text(format!(
                            "Teleport to player '{target}' not yet implemented"
                        ))
                        .color(TextColor::Named(NamedColor::Yellow)),
                    );
                } else if args.get_string("targets").is_some() {
                    ctx.send_message_component(
                        &TextComponent::text("Teleport targets not yet implemented")
                            .color(TextColor::Named(NamedColor::Yellow)),
                    );
                }
            });

        // /gamemode <mode>
        registrar
            .command("gamemode")
            .description("Change game mode")
            .arg_with(
                "mode",
                Arg::Options(vec![
                    "survival".into(),
                    "creative".into(),
                    "adventure".into(),
                    "spectator".into(),
                ]),
                Validation::Custom(
                    "Invalid gamemode. Use: survival, creative, adventure, spectator".into(),
                ),
            )
            .handler(|args, ctx| {
                let mode_str = args.get_string("mode").unwrap();
                let mode: u8 = match mode_str {
                    "survival" => 0,
                    "creative" => 1,
                    "adventure" => 2,
                    _ => 3,
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
            });

        // /say <message>
        registrar
            .command("say")
            .description("Broadcast a server message")
            .arg("message", Arg::Message)
            .handler(|args, ctx| {
                let message = args.get_string("message").unwrap_or("");
                let msg = TextComponent::text("[Server] ")
                    .color(TextColor::Named(NamedColor::LightPurple))
                    .bold(true)
                    .append(
                        TextComponent::text(message).color(TextColor::Named(NamedColor::White)),
                    );
                ctx.broadcast_message_component(&msg);
            });

        // /stop
        registrar
            .command("stop")
            .description("Stop the server")
            .handler(|_args, ctx| {
                ctx.broadcast_message_component(
                    &TextComponent::text("Server is shutting down...")
                        .color(TextColor::Named(NamedColor::Red))
                        .bold(true),
                );
                let log = ctx.logger();
                log.info("Stop command issued");
            });

        // /kick <player>
        registrar
            .command("kick")
            .description("Kick a player")
            .arg("player", Arg::Player)
            .handler(|args, ctx| {
                let target = args.get_string("player").unwrap();
                let log = ctx.logger();
                log.info(&format!("Kick issued for {target} — not yet implemented"));
                ctx.send_message_component(
                    &TextComponent::text(format!("Kick not yet implemented: {target}"))
                        .color(TextColor::Named(NamedColor::Yellow)),
                );
            });

        // /list
        registrar
            .command("list")
            .description("List connected players")
            .handler(|_args, ctx| {
                ctx.send_message_component(
                    &TextComponent::text("Player list not yet implemented")
                        .color(TextColor::Named(NamedColor::Yellow)),
                );
            });

        // /help
        registrar
            .command("help")
            .description("Show available commands")
            .handler(|_args, ctx| {
                let mut msg = TextComponent::text("Available commands:")
                    .color(TextColor::Named(NamedColor::Gold));
                let mut cmds = ctx.registered_commands();
                cmds.sort_by(|(a, _), (b, _)| a.cmp(b));
                for (name, desc) in &cmds {
                    msg = msg
                        .append(
                            TextComponent::text(format!("\n /{name}"))
                                .color(TextColor::Named(NamedColor::Yellow)),
                        )
                        .append(
                            TextComponent::text(format!(" — {desc}"))
                                .color(TextColor::Named(NamedColor::Gray)),
                        );
                }
                ctx.send_message_component(&msg);
            });
    }
}

impl Default for CommandPlugin {
    fn default() -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use basalt_api::context::ServerContext;
    use basalt_api::{EventBus, Response};
    use basalt_command::parse_command_args;
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

    fn dispatch_command(cmd: &str) -> Vec<Response> {
        let ctx = test_ctx();

        let plugin = CommandPlugin;
        let mut bus = EventBus::new();
        let mut cmds = Vec::new();
        {
            let mut registrar = PluginRegistrar::new(&mut bus, &mut cmds);
            plugin.on_enable(&mut registrar);
        }

        let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
        let name = parts[0];
        let args = parts.get(1).copied().unwrap_or("");
        if let Some(entry) = cmds.iter().find(|c| c.name == name)
            && let Ok(parsed) = parse_command_args(args, &entry.args, &entry.variants)
        {
            (entry.handler)(&parsed, &ctx);
        }
        ctx.drain_responses()
    }

    #[test]
    fn tp_coords() {
        let responses = dispatch_command("tp 10 64 -5");
        assert_eq!(responses.len(), 2);
        assert!(matches!(responses[0], Response::SendPosition { .. }));
    }

    #[test]
    fn tp_player() {
        let responses = dispatch_command("tp Steve");
        assert_eq!(responses.len(), 1);
        assert!(matches!(responses[0], Response::SendSystemChat { .. }));
    }

    #[test]
    fn gamemode_creative() {
        let responses = dispatch_command("gamemode creative");
        assert_eq!(responses.len(), 2);
        assert!(matches!(responses[0], Response::SendGameStateChange { .. }));
    }

    #[test]
    fn say_message() {
        let responses = dispatch_command("say hello world");
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::Broadcast(basalt_api::BroadcastMessage::Chat { .. })
        ));
    }

    #[test]
    fn help_command() {
        let responses = dispatch_command("help");
        assert_eq!(responses.len(), 1);
        assert!(matches!(responses[0], Response::SendSystemChat { .. }));
    }

    #[test]
    fn unknown_command_returns_empty() {
        let responses = dispatch_command("foobar");
        assert!(responses.is_empty());
    }

    #[test]
    fn default_impl() {
        let _plugin: CommandPlugin = Default::default();
        let _ = _plugin;
    }
}
