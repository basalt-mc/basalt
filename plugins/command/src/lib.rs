//! Command plugin for gameplay commands.
//!
//! Registers gameplay commands (/tp, /gamemode, /say, /help) on a
//! [`CommandRegistry`] and dispatches them when players issue commands
//! in chat. Server-level commands (stop, kick, list) are handled
//! separately by the server runtime.

pub mod commands;

use std::sync::Arc;

use basalt_api::prelude::*;
use basalt_command::CommandRegistry;

use commands::{
    GamemodeCommand, HelpCommand, KickCommand, ListCommand, SayCommand, StopCommand, TpCommand,
};

/// Gameplay command plugin.
///
/// Owns a [`CommandRegistry`] with built-in commands and dispatches
/// `CommandEvent` to the matching command handler. Unknown commands
/// receive a red error message.
pub struct CommandPlugin {
    /// Shared registry — Arc so handler closures can reference it.
    registry: Arc<CommandRegistry>,
}

impl CommandPlugin {
    /// Creates a command plugin with the default gameplay commands.
    pub fn new() -> Self {
        Self::builder().with_defaults().build()
    }

    /// Creates a builder for customizing which commands are registered.
    pub fn builder() -> CommandPluginBuilder {
        CommandPluginBuilder {
            registry: CommandRegistry::new(),
            help_entries: Vec::new(),
        }
    }
}

impl Default for CommandPlugin {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for constructing a [`CommandPlugin`] with custom commands.
pub struct CommandPluginBuilder {
    registry: CommandRegistry,
    help_entries: Vec<(String, String)>,
}

impl CommandPluginBuilder {
    /// Registers a command on the registry.
    pub fn command(mut self, cmd: impl basalt_command::Command + 'static) -> Self {
        self.help_entries
            .push((cmd.name().to_string(), cmd.description().to_string()));
        self.registry.register(cmd);
        self
    }

    /// Registers the default commands (gameplay + administration).
    pub fn with_defaults(self) -> Self {
        self.command(TpCommand)
            .command(GamemodeCommand)
            .command(SayCommand)
            .command(StopCommand)
            .command(KickCommand)
            .command(ListCommand)
    }

    /// Builds the plugin, adding `/help` with a snapshot of all
    /// registered commands.
    pub fn build(mut self) -> CommandPlugin {
        self.help_entries
            .push(("help".into(), "Show available commands".into()));
        self.registry.register(HelpCommand::new(self.help_entries));
        CommandPlugin {
            registry: Arc::new(self.registry),
        }
    }
}

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
        // Register each command from the internal registry
        // on the plugin registrar so the server collects them all.
        let registry = Arc::clone(&self.registry);
        for cmd in registry.commands() {
            let name = cmd.name();
            let desc = cmd.description();
            let reg = Arc::clone(&registry);
            let cmd_name = name.to_string();
            registrar.register_command(name, desc, move |args, ctx| {
                reg.execute(&cmd_name, args, ctx);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use basalt_api::context::ServerContext;
    use basalt_api::{EventBus, Response};
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

        // Collect commands from the plugin
        let plugin = CommandPlugin::new();
        let mut bus = EventBus::new();
        let mut cmds = Vec::new();
        {
            let mut registrar = PluginRegistrar::new(&mut bus, &mut cmds);
            plugin.on_enable(&mut registrar);
        }

        // Dispatch like the server does: find and call the handler
        let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
        let name = parts[0];
        let args = parts.get(1).copied().unwrap_or("");
        let entry = cmds.iter().find(|c| c.name == name);
        if let Some(entry) = entry {
            (entry.handler)(args, &ctx);
        }
        ctx.drain_responses()
    }

    #[test]
    fn tp_valid() {
        let responses = dispatch_command("tp 10 64 -5");
        assert_eq!(responses.len(), 2); // SendPosition + SendSystemChat
        assert!(matches!(responses[0], Response::SendPosition { .. }));
    }

    #[test]
    fn tp_invalid() {
        let responses = dispatch_command("tp");
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
        assert!(matches!(responses[0], Response::SendSystemChat { .. }));
    }

    #[test]
    fn help_lists_commands() {
        let responses = dispatch_command("help");
        assert_eq!(responses.len(), 1);
        assert!(matches!(responses[0], Response::SendSystemChat { .. }));
    }

    #[test]
    fn unknown_command_returns_empty() {
        // Unknown command handling is done by the server, not the plugin.
        // The plugin only registers known commands.
        let responses = dispatch_command("foobar");
        assert!(responses.is_empty());
    }

    #[test]
    fn builder_custom_command() {
        struct PingCmd;
        impl basalt_command::Command for PingCmd {
            fn name(&self) -> &str {
                "ping"
            }
            fn description(&self) -> &str {
                "Pong"
            }
            fn execute(&self, _args: &str, ctx: &ServerContext) {
                ctx.send_message("Pong!");
            }
        }

        let plugin = CommandPlugin::builder().command(PingCmd).build();
        assert_eq!(plugin.registry.len(), 2); // ping + help
    }

    #[test]
    fn default_has_all_commands() {
        let plugin = CommandPlugin::new();
        // tp, gamemode, say, stop, kick, list, help
        assert_eq!(plugin.registry.len(), 7);
    }
}
