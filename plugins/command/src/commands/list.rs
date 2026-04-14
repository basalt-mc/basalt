//! `/list` — list connected players.

use basalt_api::context::ServerContext;
use basalt_command::Command;
use basalt_types::{NamedColor, TextColor, TextComponent};

/// Lists all connected players.
pub struct ListCommand;

impl Command for ListCommand {
    fn name(&self) -> &str {
        "list"
    }

    fn description(&self) -> &str {
        "List connected players"
    }

    fn execute(&self, _args: &str, ctx: &ServerContext) {
        // TODO: access player registry via ServerContext once exposed
        ctx.send_message_component(
            &TextComponent::text("Player list not yet implemented")
                .color(TextColor::Named(NamedColor::Yellow)),
        );
    }
}
