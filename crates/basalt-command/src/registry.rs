//! Command registry for looking up and executing commands by name.
//!
//! The [`CommandRegistry`] stores registered commands and dispatches
//! execution by name. It is typically owned by a `CommandPlugin`
//! and shared with event handler closures via `Arc`.

use std::collections::HashMap;

use basalt_api::context::ServerContext;

use crate::Command;

/// A registry of named commands.
///
/// Commands are registered at startup and looked up by name during
/// play. The registry is immutable after construction — commands
/// cannot be added or removed at runtime.
pub struct CommandRegistry {
    commands: HashMap<String, Box<dyn Command>>,
}

impl CommandRegistry {
    /// Creates an empty command registry.
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    /// Registers a command. Overwrites any existing command with
    /// the same name.
    pub fn register(&mut self, command: impl Command + 'static) {
        let name = command.name().to_string();
        self.commands.insert(name, Box::new(command));
    }

    /// Executes a command by name.
    ///
    /// Returns `true` if the command was found and executed,
    /// `false` if no command with that name exists.
    pub fn execute(&self, name: &str, args: &str, ctx: &ServerContext) -> bool {
        if let Some(cmd) = self.commands.get(name) {
            cmd.execute(args, ctx);
            true
        } else {
            false
        }
    }

    /// Returns an iterator over all registered commands.
    pub fn commands(&self) -> impl Iterator<Item = &dyn Command> {
        self.commands.values().map(|c| c.as_ref())
    }

    /// Looks up a command by name.
    pub fn get(&self, name: &str) -> Option<&dyn Command> {
        self.commands.get(name).map(|c| c.as_ref())
    }

    /// Returns the number of registered commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Returns true if no commands are registered.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use basalt_types::Uuid;

    use super::*;

    struct PingCommand;

    impl Command for PingCommand {
        fn name(&self) -> &str {
            "ping"
        }
        fn description(&self) -> &str {
            "Responds with pong"
        }
        fn execute(&self, _args: &str, ctx: &ServerContext) {
            ctx.send_message("Pong!");
        }
    }

    struct EchoCommand;

    impl Command for EchoCommand {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "Echoes the arguments"
        }
        fn execute(&self, args: &str, ctx: &ServerContext) {
            ctx.send_message(args);
        }
    }

    fn test_world() -> &'static basalt_world::World {
        use std::sync::OnceLock;
        static WORLD: OnceLock<basalt_world::World> = OnceLock::new();
        WORLD.get_or_init(|| basalt_world::World::new_memory(42))
    }

    fn test_ctx() -> ServerContext {
        ServerContext::new(test_world(), Uuid::default(), 1, "Steve".into())
    }

    #[test]
    fn register_and_execute() {
        let mut registry = CommandRegistry::new();
        registry.register(PingCommand);

        let ctx = test_ctx();
        assert!(registry.execute("ping", "", &ctx));

        let responses = ctx.drain_responses();
        assert_eq!(responses.len(), 1);
    }

    #[test]
    fn unknown_command_returns_false() {
        let registry = CommandRegistry::new();
        let ctx = test_ctx();
        assert!(!registry.execute("nonexistent", "", &ctx));
        assert!(ctx.drain_responses().is_empty());
    }

    #[test]
    fn multiple_commands() {
        let mut registry = CommandRegistry::new();
        registry.register(PingCommand);
        registry.register(EchoCommand);

        assert_eq!(registry.len(), 2);
        assert!(!registry.is_empty());
    }

    #[test]
    fn get_command() {
        let mut registry = CommandRegistry::new();
        registry.register(PingCommand);

        assert!(registry.get("ping").is_some());
        assert_eq!(registry.get("ping").unwrap().name(), "ping");
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn iterate_commands() {
        let mut registry = CommandRegistry::new();
        registry.register(PingCommand);
        registry.register(EchoCommand);

        let names: Vec<&str> = registry.commands().map(|c| c.name()).collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"ping"));
        assert!(names.contains(&"echo"));
    }

    #[test]
    fn empty_registry() {
        let registry = CommandRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn default_is_empty() {
        let registry = CommandRegistry::default();
        assert!(registry.is_empty());
    }

    #[test]
    fn execute_with_args() {
        let mut registry = CommandRegistry::new();
        registry.register(EchoCommand);

        let ctx = test_ctx();
        registry.execute("echo", "hello world", &ctx);

        let responses = ctx.drain_responses();
        assert_eq!(responses.len(), 1);
    }
}
