//! Plugin trait and registration API.
//!
//! Every server feature — built-in or external — implements the
//! [`Plugin`] trait. Plugins register event handlers and commands
//! during [`on_enable`](Plugin::on_enable).

use basalt_command::{Arg, CommandArg, CommandArgs, Validation};
use basalt_core::Context;
use basalt_events::{BusKind, Event, EventBus, EventRouting, Stage};

use crate::context::ServerContext;

/// A server plugin that registers event handlers and lifecycle hooks.
pub trait Plugin: Send + Sync + 'static {
    /// Returns the plugin's identity metadata.
    fn metadata(&self) -> PluginMetadata;

    /// Called when the plugin is enabled. Register event handlers
    /// and commands here.
    fn on_enable(&self, registrar: &mut PluginRegistrar);

    /// Called when the plugin is disabled (server shutdown).
    fn on_disable(&self) {}
}

/// Identity metadata for a plugin.
#[derive(Debug, Clone)]
pub struct PluginMetadata {
    /// Human-readable plugin name.
    pub name: &'static str,
    /// Semver version string.
    pub version: &'static str,
    /// Optional author name.
    pub author: Option<&'static str>,
    /// Plugin names that must be loaded before this plugin.
    pub dependencies: &'static [&'static str],
}

/// Handler function type for commands with typed arguments.
pub type CommandHandler = Box<dyn Fn(&CommandArgs, &dyn Context) + Send + Sync>;

/// A registered command entry.
pub struct CommandEntry {
    /// Command name without the leading `/`.
    pub name: String,
    /// Short description for help listing.
    pub description: String,
    /// Single argument list (used when `variants` is empty).
    pub args: Vec<CommandArg>,
    /// Multiple argument variants for polymorphic commands.
    pub variants: Vec<Vec<CommandArg>>,
    /// The command handler function.
    pub handler: CommandHandler,
}

/// Plugin registration interface for events and commands.
///
/// Holds mutable references to both the network and game event buses.
/// Handler registration is routed automatically based on the event
/// type's [`EventRouting::BUS`] constant — plugins do not specify
/// which loop handles their events.
pub struct PluginRegistrar<'a> {
    /// Event bus for the network loop (movement, chat, commands).
    network_bus: &'a mut EventBus,
    /// Event bus for the game loop (blocks, world mutations).
    game_bus: &'a mut EventBus,
    /// Collected command entries.
    commands: &'a mut Vec<CommandEntry>,
}

impl<'a> PluginRegistrar<'a> {
    /// Creates a new registrar with dual event buses.
    pub fn new(
        network_bus: &'a mut EventBus,
        game_bus: &'a mut EventBus,
        commands: &'a mut Vec<CommandEntry>,
    ) -> Self {
        Self {
            network_bus,
            game_bus,
            commands,
        }
    }

    /// Registers an event handler on the correct bus.
    ///
    /// The target bus is determined at compile time by `E::BUS`:
    /// - [`BusKind::Network`] → network loop bus
    /// - [`BusKind::Game`] → game loop bus
    pub fn on<E>(
        &mut self,
        stage: Stage,
        priority: i32,
        handler: impl Fn(&mut E, &ServerContext) + Send + Sync + 'static,
    ) where
        E: Event + EventRouting + 'static,
    {
        match E::BUS {
            BusKind::Network => self
                .network_bus
                .on::<E, ServerContext>(stage, priority, handler),
            BusKind::Game => self
                .game_bus
                .on::<E, ServerContext>(stage, priority, handler),
        }
    }

    /// Starts building a command with typed arguments.
    pub fn command(&mut self, name: &str) -> CommandBuilder<'_, 'a> {
        CommandBuilder {
            registrar: self,
            name: name.to_string(),
            description: String::new(),
            args: Vec::new(),
            variants: Vec::new(),
        }
    }
}

/// Fluent builder for registering a command with typed arguments.
pub struct CommandBuilder<'r, 'a> {
    registrar: &'r mut PluginRegistrar<'a>,
    name: String,
    description: String,
    args: Vec<CommandArg>,
    variants: Vec<Vec<CommandArg>>,
}

impl<'r, 'a> CommandBuilder<'r, 'a> {
    /// Sets the command description (shown in /help).
    pub fn description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    /// Adds a required argument with default validation.
    pub fn arg(mut self, name: &str, arg_type: Arg) -> Self {
        self.args.push(CommandArg {
            name: name.to_string(),
            arg_type,
            validation: Validation::Auto,
            required: true,
        });
        self
    }

    /// Adds a required argument with custom validation.
    pub fn arg_with(mut self, name: &str, arg_type: Arg, validation: Validation) -> Self {
        self.args.push(CommandArg {
            name: name.to_string(),
            arg_type,
            validation,
            required: true,
        });
        self
    }

    /// Adds an optional argument with default validation.
    pub fn optional_arg(mut self, name: &str, arg_type: Arg) -> Self {
        self.args.push(CommandArg {
            name: name.to_string(),
            arg_type,
            validation: Validation::Auto,
            required: false,
        });
        self
    }

    /// Adds a variant for polymorphic commands.
    ///
    /// Each variant is a separate argument list. The parser tries
    /// variants in order and uses the first one that succeeds.
    pub fn variant(mut self, build: impl FnOnce(VariantBuilder) -> VariantBuilder) -> Self {
        let builder = build(VariantBuilder { args: Vec::new() });
        self.variants.push(builder.args);
        self
    }

    /// Sets the handler and registers the command.
    pub fn handler(self, handler: impl Fn(&CommandArgs, &dyn Context) + Send + Sync + 'static) {
        self.registrar.commands.push(CommandEntry {
            name: self.name,
            description: self.description,
            args: self.args,
            variants: self.variants,
            handler: Box::new(handler),
        });
    }
}

/// Builder for a single variant of a polymorphic command.
pub struct VariantBuilder {
    args: Vec<CommandArg>,
}

impl VariantBuilder {
    /// Adds a required argument to this variant.
    pub fn arg(mut self, name: &str, arg_type: Arg) -> Self {
        self.args.push(CommandArg {
            name: name.to_string(),
            arg_type,
            validation: Validation::Auto,
            required: true,
        });
        self
    }

    /// Adds a required argument with custom validation.
    pub fn arg_with(mut self, name: &str, arg_type: Arg, validation: Validation) -> Self {
        self.args.push(CommandArg {
            name: name.to_string(),
            arg_type,
            validation,
            required: true,
        });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestPlugin;

    impl Plugin for TestPlugin {
        fn metadata(&self) -> PluginMetadata {
            PluginMetadata {
                name: "test",
                version: "0.1.0",
                author: Some("Test"),
                dependencies: &[],
            }
        }

        fn on_enable(&self, _registrar: &mut PluginRegistrar) {}
    }

    #[test]
    fn plugin_metadata() {
        let meta = TestPlugin.metadata();
        assert_eq!(meta.name, "test");
    }

    #[test]
    fn plugin_on_disable_default_is_noop() {
        TestPlugin.on_disable();
    }

    #[test]
    fn registrar_routes_to_correct_bus() {
        use crate::events::{BlockBrokenEvent, ChatMessageEvent};

        let mut network_bus = EventBus::new();
        let mut game_bus = EventBus::new();
        let mut commands = Vec::new();
        {
            let mut registrar =
                PluginRegistrar::new(&mut network_bus, &mut game_bus, &mut commands);
            registrar.on::<ChatMessageEvent>(Stage::Post, 0, |_event, _ctx| {});
            registrar.on::<BlockBrokenEvent>(Stage::Process, 0, |_event, _ctx| {});
        }
        assert_eq!(network_bus.handler_count(), 1);
        assert_eq!(game_bus.handler_count(), 1);
    }

    #[test]
    fn command_builder_with_args() {
        let mut network_bus = EventBus::new();
        let mut game_bus = EventBus::new();
        let mut commands = Vec::new();
        {
            let mut registrar =
                PluginRegistrar::new(&mut network_bus, &mut game_bus, &mut commands);
            registrar
                .command("tp")
                .description("Teleport")
                .arg("x", Arg::Double)
                .arg("y", Arg::Double)
                .arg("z", Arg::Double)
                .handler(|_args, _ctx| {});
        }
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "tp");
        assert_eq!(commands[0].args.len(), 3);
        assert!(commands[0].variants.is_empty());
    }

    #[test]
    fn command_builder_with_variants() {
        let mut network_bus = EventBus::new();
        let mut game_bus = EventBus::new();
        let mut commands = Vec::new();
        {
            let mut registrar =
                PluginRegistrar::new(&mut network_bus, &mut game_bus, &mut commands);
            registrar
                .command("tp")
                .description("Teleport")
                .variant(|v| v.arg("destination", Arg::Player))
                .variant(|v| {
                    v.arg("x", Arg::Double)
                        .arg("y", Arg::Double)
                        .arg("z", Arg::Double)
                })
                .handler(|_args, _ctx| {});
        }
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].variants.len(), 2);
        assert_eq!(commands[0].variants[0].len(), 1); // player
        assert_eq!(commands[0].variants[1].len(), 3); // x y z
    }

    #[test]
    fn command_no_args() {
        let mut network_bus = EventBus::new();
        let mut game_bus = EventBus::new();
        let mut commands = Vec::new();
        {
            let mut registrar =
                PluginRegistrar::new(&mut network_bus, &mut game_bus, &mut commands);
            registrar
                .command("help")
                .description("Show help")
                .handler(|_args, _ctx| {});
        }
        assert_eq!(commands.len(), 1);
        assert!(commands[0].args.is_empty());
        assert!(commands[0].variants.is_empty());
    }
}
