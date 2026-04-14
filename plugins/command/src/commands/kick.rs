//! `/kick <player>` — disconnect a player.

use basalt_api::context::ServerContext;
use basalt_command::Command;
use basalt_types::{NamedColor, TextColor, TextComponent};

/// Disconnects a player from the server.
pub struct KickCommand;

impl Command for KickCommand {
    fn name(&self) -> &str {
        "kick"
    }

    fn description(&self) -> &str {
        "Kick a player"
    }

    fn execute(&self, args: &str, ctx: &ServerContext) {
        let target = args.trim();
        if target.is_empty() {
            ctx.send_message_component(
                &TextComponent::text("Usage: /kick <player>")
                    .color(TextColor::Named(NamedColor::Red)),
            );
            return;
        }
        // TODO: implement actual kick via a Response variant
        let log = ctx.logger();
        log.info(&format!(
            "Kick command issued for {target} — not yet implemented"
        ));
        ctx.send_message_component(
            &TextComponent::text(format!("Kick not yet implemented: {target}"))
                .color(TextColor::Named(NamedColor::Yellow)),
        );
    }
}
