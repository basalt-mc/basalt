//! Command registry for looking up and executing commands by name.

use std::collections::HashMap;

use crate::context::Context;

use super::args::CommandArgs;
use super::dispatch::Command;

/// A registry of named commands.
///
/// Commands are registered at startup and looked up by name during
/// play. The registry is immutable after construction.
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

    /// Executes a command by name with parsed arguments.
    ///
    /// Returns `true` if the command was found and executed.
    pub fn execute(&self, name: &str, args: &CommandArgs, ctx: &dyn Context) -> bool {
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
    use crate::testing::NoopContext;

    use super::super::args::Arg;
    use super::*;

    struct PingCommand;
    impl Command for PingCommand {
        fn name(&self) -> &str {
            "ping"
        }
        fn description(&self) -> &str {
            "Responds with pong"
        }
        fn execute(&self, _args: &CommandArgs, ctx: &dyn Context) {
            ctx.chat().send("Pong!");
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
        fn execute(&self, args: &CommandArgs, ctx: &dyn Context) {
            ctx.chat().send(args.raw());
        }
    }

    #[test]
    fn register_and_execute() {
        let mut registry = CommandRegistry::new();
        registry.register(PingCommand);

        let args = CommandArgs::new(String::new());
        let ctx = NoopContext;
        assert!(registry.execute("ping", &args, &ctx));
    }

    #[test]
    fn unknown_command_returns_false() {
        let registry = CommandRegistry::new();
        let args = CommandArgs::new(String::new());
        assert!(!registry.execute("nonexistent", &args, &NoopContext));
    }

    #[test]
    fn multiple_commands() {
        let mut registry = CommandRegistry::new();
        registry.register(PingCommand);
        registry.register(EchoCommand);
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn get_command() {
        let mut registry = CommandRegistry::new();
        registry.register(PingCommand);
        assert!(registry.get("ping").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn iterate_commands() {
        let mut registry = CommandRegistry::new();
        registry.register(PingCommand);
        registry.register(EchoCommand);
        let names: Vec<&str> = registry.commands().map(|c| c.name()).collect();
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn empty_registry() {
        let registry = CommandRegistry::new();
        assert!(registry.is_empty());
    }

    #[test]
    fn default_is_empty() {
        let registry = CommandRegistry::default();
        assert!(registry.is_empty());
    }

    #[test]
    fn parse_and_execute() {
        let mut registry = CommandRegistry::new();
        registry.register(EchoCommand);

        let schema = vec![super::super::args::CommandArg {
            name: "msg".into(),
            arg_type: Arg::Message,
            validation: super::super::args::Validation::Auto,
            required: true,
        }];
        let args = super::super::args::parse_args("hello world", &schema).unwrap();
        registry.execute("echo", &args, &NoopContext);
    }
}
