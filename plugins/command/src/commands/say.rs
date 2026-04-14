//! `/say <message>` — broadcast a server message.

use basalt_api::context::ServerContext;
use basalt_command::Command;
use basalt_types::{NamedColor, TextColor, TextComponent};

/// Broadcasts a server-prefixed message to the current player.
pub struct SayCommand;

impl Command for SayCommand {
    fn name(&self) -> &str {
        "say"
    }

    fn description(&self) -> &str {
        "Broadcast a server message"
    }

    fn execute(&self, args: &str, ctx: &ServerContext) {
        let msg = TextComponent::text("[Server] ")
            .color(TextColor::Named(NamedColor::LightPurple))
            .bold(true)
            .append(TextComponent::text(args).color(TextColor::Named(NamedColor::White)));
        ctx.send_message_component(&msg);
    }
}
