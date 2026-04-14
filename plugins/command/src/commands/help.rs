//! `/help` — list all available commands.

use basalt_api::context::ServerContext;
use basalt_command::Command;
use basalt_types::{NamedColor, TextColor, TextComponent};

/// Lists all registered commands with their descriptions.
///
/// The command list is captured as a snapshot at build time —
/// commands registered after `HelpCommand` is created won't appear.
pub struct HelpCommand {
    /// Pre-built (name, description) pairs, sorted by name.
    entries: Vec<(String, String)>,
}

impl HelpCommand {
    /// Creates a help command with the given command entries.
    pub fn new(entries: Vec<(String, String)>) -> Self {
        let mut entries = entries;
        entries.sort_by(|(a, _), (b, _)| a.cmp(b));
        Self { entries }
    }
}

impl Command for HelpCommand {
    fn name(&self) -> &str {
        "help"
    }

    fn description(&self) -> &str {
        "Show available commands"
    }

    fn execute(&self, _args: &str, ctx: &ServerContext) {
        let mut msg =
            TextComponent::text("Available commands:").color(TextColor::Named(NamedColor::Gold));

        for (name, desc) in &self.entries {
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
    }
}
