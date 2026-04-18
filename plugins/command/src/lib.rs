//! Command plugin for gameplay and administration commands.
//!
//! Registers all in-game commands via the `PluginRegistrar` builder
//! API. Commands use typed arguments with auto-validation and
//! tab-completion.

use basalt_api::command::{Arg, Validation};
use basalt_api::prelude::*;
use basalt_api::types::{NamedColor, TextColor, TextComponent};

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
                if let Some((x, y, z)) = args.get_vec3("location") {
                    ctx.player()
                        .teleport(x, y, z, ctx.player().yaw(), ctx.player().pitch());
                    ctx.chat().send_component(
                        &TextComponent::text(format!("Teleported to {x}, {y}, {z}"))
                            .color(TextColor::Named(NamedColor::Green)),
                    );
                } else if let Some(target) = args.get_string("destination") {
                    ctx.chat().send_component(
                        &TextComponent::text(format!(
                            "Teleport to player '{target}' not yet implemented"
                        ))
                        .color(TextColor::Named(NamedColor::Yellow)),
                    );
                } else if args.get_string("targets").is_some() {
                    ctx.chat().send_component(
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
                let mode = match mode_str {
                    "survival" => Gamemode::Survival,
                    "creative" => Gamemode::Creative,
                    "adventure" => Gamemode::Adventure,
                    _ => Gamemode::Spectator,
                };
                ctx.player().set_gamemode(mode);
                ctx.chat().send_component(
                    &TextComponent::text(format!("Game mode set to {mode}"))
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
                ctx.chat().broadcast_component(&msg);
            });

        // /stop
        registrar
            .command("stop")
            .description("Stop the server")
            .handler(|_args, ctx| {
                ctx.chat().broadcast_component(
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
                log.info(format_args!(
                    "Kick issued for {target} — not yet implemented"
                ));
                ctx.chat().send_component(
                    &TextComponent::text(format!("Kick not yet implemented: {target}"))
                        .color(TextColor::Named(NamedColor::Yellow)),
                );
            });

        // /list
        registrar
            .command("list")
            .description("List connected players")
            .handler(|_args, ctx| {
                ctx.chat().send_component(
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
                let mut cmds = ctx.player().registered_commands();
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
                ctx.chat().send_component(&msg);
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
    use basalt_testkit::{DispatchResult, PluginTestHarness};

    use super::*;

    fn harness() -> PluginTestHarness {
        let mut h = PluginTestHarness::new();
        h.register(CommandPlugin);
        h
    }

    fn dispatch_command(cmd: &str) -> DispatchResult {
        harness().dispatch_command(cmd)
    }

    #[test]
    fn tp_coords() {
        let result = dispatch_command("tp 10 64 -5");
        assert_eq!(result.len(), 2);
        assert!(result.has_teleport());
    }

    #[test]
    fn tp_player() {
        let result = dispatch_command("tp Steve");
        assert_eq!(result.len(), 1);
        assert!(result.has_system_chat());
    }

    #[test]
    fn gamemode_creative() {
        let result = dispatch_command("gamemode creative");
        assert_eq!(result.len(), 2);
        assert!(result.has_game_state_change());
    }

    #[test]
    fn say_message() {
        let result = dispatch_command("say hello world");
        assert_eq!(result.len(), 1);
        assert!(result.has_chat_broadcast());
    }

    #[test]
    fn help_command() {
        let result = dispatch_command("help");
        assert_eq!(result.len(), 1);
        assert!(result.has_system_chat());
    }

    #[test]
    fn stop_command() {
        let result = dispatch_command("stop");
        assert_eq!(result.len(), 1);
        assert!(result.has_chat_broadcast());
    }

    #[test]
    fn kick_command() {
        let result = dispatch_command("kick Steve");
        assert_eq!(result.len(), 1);
        assert!(result.has_system_chat());
    }

    #[test]
    fn list_command() {
        let result = dispatch_command("list");
        assert_eq!(result.len(), 1);
        assert!(result.has_system_chat());
    }

    #[test]
    fn tp_invalid_coords() {
        let result = dispatch_command("tp abc def ghi");
        assert_eq!(result.len(), 1);
        assert!(result.has_system_chat());
    }

    #[test]
    fn gamemode_survival() {
        let result = dispatch_command("gamemode survival");
        assert_eq!(result.len(), 2);
        assert!(result.has_game_state_change());
    }

    #[test]
    fn unknown_command_returns_empty() {
        let result = dispatch_command("foobar");
        assert!(result.is_empty());
    }
}
