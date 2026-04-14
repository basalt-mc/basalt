//! `/gamemode <mode>` — change the player's gamemode.

use basalt_api::context::ServerContext;
use basalt_command::Command;
use basalt_types::{NamedColor, TextColor, TextComponent};

/// Changes the player's gamemode.
pub struct GamemodeCommand;

impl Command for GamemodeCommand {
    fn name(&self) -> &str {
        "gamemode"
    }

    fn description(&self) -> &str {
        "Change game mode"
    }

    fn execute(&self, args: &str, ctx: &ServerContext) {
        let mode: u8 = match args.trim() {
            "survival" | "0" => 0,
            "creative" | "1" => 1,
            "adventure" | "2" => 2,
            "spectator" | "3" => 3,
            _ => {
                ctx.send_message_component(
                    &TextComponent::text(
                        "Usage: /gamemode <survival|creative|adventure|spectator>",
                    )
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
}
