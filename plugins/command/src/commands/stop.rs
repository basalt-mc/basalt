//! `/stop` — stop the server.

use basalt_api::context::ServerContext;
use basalt_command::Command;
use basalt_types::{NamedColor, TextColor, TextComponent};

/// Stops the server gracefully.
pub struct StopCommand;

impl Command for StopCommand {
    fn name(&self) -> &str {
        "stop"
    }

    fn description(&self) -> &str {
        "Stop the server"
    }

    fn execute(&self, _args: &str, ctx: &ServerContext) {
        ctx.broadcast_message_component(
            &TextComponent::text("Server is shutting down...")
                .color(TextColor::Named(NamedColor::Red))
                .bold(true),
        );
        // TODO: trigger graceful shutdown via a Response variant
        let log = ctx.logger();
        log.info("Stop command issued — graceful shutdown not yet implemented");
    }
}
