//! `/tp <x> <y> <z>` — teleport the player to coordinates.

use basalt_api::context::ServerContext;
use basalt_command::Command;
use basalt_types::{NamedColor, TextColor, TextComponent};

/// Teleports the player to the given coordinates.
pub struct TpCommand;

impl Command for TpCommand {
    fn name(&self) -> &str {
        "tp"
    }

    fn description(&self) -> &str {
        "Teleport to coordinates"
    }

    fn execute(&self, args: &str, ctx: &ServerContext) {
        let coords: Vec<&str> = args.split_whitespace().collect();
        if coords.len() != 3 {
            ctx.send_message_component(
                &TextComponent::text("Usage: /tp <x> <y> <z>")
                    .color(TextColor::Named(NamedColor::Red)),
            );
            return;
        }

        let Ok(x) = coords[0].parse::<f64>() else {
            ctx.send_message_component(
                &TextComponent::text("Invalid x coordinate")
                    .color(TextColor::Named(NamedColor::Red)),
            );
            return;
        };
        let Ok(y) = coords[1].parse::<f64>() else {
            ctx.send_message_component(
                &TextComponent::text("Invalid y coordinate")
                    .color(TextColor::Named(NamedColor::Red)),
            );
            return;
        };
        let Ok(z) = coords[2].parse::<f64>() else {
            ctx.send_message_component(
                &TextComponent::text("Invalid z coordinate")
                    .color(TextColor::Named(NamedColor::Red)),
            );
            return;
        };

        ctx.teleport(x, y, z, 0.0, 0.0);
        ctx.send_message_component(
            &TextComponent::text(format!("Teleported to {x}, {y}, {z}"))
                .color(TextColor::Named(NamedColor::Green)),
        );
    }
}
