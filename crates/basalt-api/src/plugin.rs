//! Plugin trait and registration API.
//!
//! Every server feature — built-in or external — implements the
//! [`Plugin`] trait. Plugins register event handlers during
//! [`on_enable`](Plugin::on_enable) and clean up during
//! [`on_disable`](Plugin::on_disable).

use basalt_events::{Event, EventBus, Stage};

use crate::context::ServerContext;

/// A server plugin that registers event handlers and lifecycle hooks.
///
/// Plugins are compile-time crate dependencies. Each plugin implements
/// this trait and is added to the server via `ServerBuilder::add_plugin`.
/// Built-in plugins (chat, world, block interaction) implement this
/// same trait — there is no backdoor API.
///
/// # Lifecycle
///
/// 1. `metadata()` — called to read plugin identity
/// 2. `on_enable(&mut EventRegistrar)` — registers event handlers
/// 3. Server runs (handlers fire on game events)
/// 4. `on_disable()` — cleanup on shutdown
///
/// # Example
///
/// ```ignore
/// use basalt_api::prelude::*;
///
/// pub struct MotdPlugin;
///
/// impl Plugin for MotdPlugin {
///     fn metadata(&self) -> PluginMetadata {
///         PluginMetadata {
///             name: "motd",
///             version: "0.1.0",
///             author: Some("Community"),
///             dependencies: &[],
///         }
///     }
///
///     fn on_enable(&self, registrar: &mut EventRegistrar) {
///         registrar.on::<PlayerJoinedEvent>(Stage::Post, 0, |_event, ctx| {
///             ctx.send_message("Welcome to the server!");
///         });
///     }
/// }
/// ```
pub trait Plugin: Send + Sync + 'static {
    /// Returns the plugin's identity metadata.
    fn metadata(&self) -> PluginMetadata;

    /// Called when the plugin is enabled. Register event handlers here.
    ///
    /// The `registrar` provides typed methods for subscribing to events
    /// at specific stages with priority ordering. This is the only way
    /// to register handlers.
    fn on_enable(&self, registrar: &mut PluginRegistrar);

    /// Called when the plugin is disabled (server shutdown).
    ///
    /// Override to clean up resources, flush caches, or log shutdown.
    /// Default implementation is a no-op.
    fn on_disable(&self) {}
}

/// Identity metadata for a plugin.
///
/// Describes the plugin's name, version, author, and load-order
/// dependencies. Used for logging, diagnostics, and topological
/// sorting of plugin initialization.
#[derive(Debug, Clone)]
pub struct PluginMetadata {
    /// Human-readable plugin name. Used as an identifier for
    /// dependency resolution and logging.
    pub name: &'static str,
    /// Semver version string.
    pub version: &'static str,
    /// Optional author name.
    pub author: Option<&'static str>,
    /// Plugin names that must be loaded before this plugin.
    ///
    /// The server sorts plugins topologically by their dependencies
    /// before calling `on_enable`. An empty slice means no ordering
    /// constraints.
    pub dependencies: &'static [&'static str],
}

/// Handler function type for commands.
pub type CommandHandler = Box<dyn Fn(&str, &ServerContext) + Send + Sync>;

/// A registered command entry (name, description, handler).
pub struct CommandEntry {
    /// Command name without the leading `/`.
    pub name: String,
    /// Short description for help listing.
    pub description: String,
    /// The command handler function.
    pub handler: CommandHandler,
}

/// Plugin registration interface for events and commands.
///
/// Passed to [`Plugin::on_enable`] at startup. Plugins use it to
/// register event handlers and commands. After all plugins are
/// enabled, the server collects registered commands to build the
/// `DeclareCommands` packet and the command dispatch table.
pub struct PluginRegistrar<'a> {
    bus: &'a mut EventBus,
    commands: &'a mut Vec<CommandEntry>,
}

impl<'a> PluginRegistrar<'a> {
    /// Creates a new registrar wrapping the given event bus and
    /// command list.
    pub fn new(bus: &'a mut EventBus, commands: &'a mut Vec<CommandEntry>) -> Self {
        Self { bus, commands }
    }

    /// Registers an event handler for event type `E` at the given stage.
    ///
    /// Lower priority values run first within the same stage.
    /// The handler receives a mutable reference to the event and
    /// a shared reference to [`ServerContext`].
    pub fn on<E>(
        &mut self,
        stage: Stage,
        priority: i32,
        handler: impl Fn(&mut E, &ServerContext) + Send + Sync + 'static,
    ) where
        E: Event + 'static,
    {
        self.bus.on::<E, ServerContext>(stage, priority, handler);
    }

    /// Registers a command that players can invoke with `/<name>`.
    ///
    /// The handler receives the argument string (everything after
    /// the command name) and the server context.
    ///
    /// # Example
    ///
    /// ```ignore
    /// registrar.register_command("home", "Teleport to spawn", |args, ctx| {
    ///     ctx.teleport(0.0, 64.0, 0.0, 0.0, 0.0);
    ///     ctx.send_message("Teleported home!");
    /// });
    /// ```
    pub fn register_command(
        &mut self,
        name: &str,
        description: &str,
        handler: impl Fn(&str, &ServerContext) + Send + Sync + 'static,
    ) {
        self.commands.push(CommandEntry {
            name: name.to_string(),
            description: description.to_string(),
            handler: Box::new(handler),
        });
    }
}

/// Backward-compatible alias.
#[deprecated(note = "renamed to PluginRegistrar")]
pub type EventRegistrar<'a> = PluginRegistrar<'a>;

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

        fn on_enable(&self, _registrar: &mut PluginRegistrar) {
            // no-op for metadata test
        }
    }

    #[test]
    fn plugin_metadata() {
        let plugin = TestPlugin;
        let meta = plugin.metadata();
        assert_eq!(meta.name, "test");
        assert_eq!(meta.version, "0.1.0");
        assert_eq!(meta.author, Some("Test"));
        assert!(meta.dependencies.is_empty());
    }

    #[test]
    fn plugin_on_disable_default_is_noop() {
        let plugin = TestPlugin;
        plugin.on_disable(); // should not panic
    }

    #[test]
    fn registrar_registers_handler() {
        use crate::events::ChatMessageEvent;

        let mut bus = EventBus::new();
        let mut commands = Vec::new();
        {
            let mut registrar = PluginRegistrar::new(&mut bus, &mut commands);
            registrar.on::<ChatMessageEvent>(Stage::Post, 0, |_event, _ctx| {});
        }
        assert_eq!(bus.handler_count(), 1);
    }

    #[test]
    fn registrar_registers_command() {
        let mut bus = EventBus::new();
        let mut commands = Vec::new();
        {
            let mut registrar = PluginRegistrar::new(&mut bus, &mut commands);
            registrar.register_command("test", "A test command", |_args, _ctx| {});
        }
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "test");
        assert_eq!(commands[0].description, "A test command");
    }
}
