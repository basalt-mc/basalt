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
    fn on_enable(&self, registrar: &mut EventRegistrar);

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

/// Typed registration interface for event handlers.
///
/// Wraps [`EventBus`] and locks the context type to [`ServerContext`].
/// Plugins use this to subscribe handlers without accessing the bus
/// directly, which prevents registering handlers with a wrong context
/// type.
pub struct EventRegistrar<'a> {
    bus: &'a mut EventBus,
}

impl<'a> EventRegistrar<'a> {
    /// Creates a new registrar wrapping the given event bus.
    pub fn new(bus: &'a mut EventBus) -> Self {
        Self { bus }
    }

    /// Registers a handler for event type `E` at the given stage.
    ///
    /// Lower priority values run first within the same stage.
    /// The handler receives a mutable reference to the event and
    /// a shared reference to [`ServerContext`].
    ///
    /// # Example
    ///
    /// ```ignore
    /// registrar.on::<BlockBrokenEvent>(Stage::Validate, 0, |event, ctx| {
    ///     // Check permissions, optionally cancel
    ///     if !can_build(ctx.player_uuid()) {
    ///         event.cancel();
    ///     }
    /// });
    /// ```
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

        fn on_enable(&self, _registrar: &mut EventRegistrar) {
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
        {
            let mut registrar = EventRegistrar::new(&mut bus);
            registrar.on::<ChatMessageEvent>(Stage::Post, 0, |_event, _ctx| {});
        }
        assert_eq!(bus.handler_count(), 1);
    }
}
