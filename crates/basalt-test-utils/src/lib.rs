//! Shared test helpers for Basalt plugin and server tests.
//!
//! Provides [`PluginTestHarness`] to eliminate the duplicated test
//! scaffolding (world creation, event bus, plugin registration, dispatch)
//! that appears in every plugin's test module.

use std::sync::Arc;

use basalt_api::context::ServerContext;
use basalt_api::plugin::PluginRegistrar;
use basalt_api::{EventBus, Plugin, Response};
use basalt_events::Event;
use basalt_types::Uuid;
use basalt_world::World;

/// Test harness that encapsulates the common plugin test setup.
///
/// Creates a world, event bus, and server context, then registers a
/// plugin and dispatches events — all in a few method calls instead
/// of 10+ lines of boilerplate per test.
///
/// # Example
///
/// ```ignore
/// let mut harness = PluginTestHarness::new();
/// harness.register(MyPlugin);
/// let mut event = SomeEvent { ... };
/// let responses = harness.dispatch(&mut event);
/// assert_eq!(responses.len(), 1);
/// ```
pub struct PluginTestHarness {
    /// Shared world instance for the test.
    world: Arc<World>,
    /// Event bus with registered handlers.
    bus: EventBus,
    /// Collected command entries (not used in most tests, but needed for registration).
    commands: Vec<basalt_api::CommandEntry>,
}

impl PluginTestHarness {
    /// Creates a new test harness with a memory-only noise world (seed 42).
    pub fn new() -> Self {
        Self {
            world: Arc::new(World::new_memory(42)),
            bus: EventBus::new(),
            commands: Vec::new(),
        }
    }

    /// Creates a new test harness with the given world.
    pub fn with_world(world: Arc<World>) -> Self {
        Self {
            world,
            bus: EventBus::new(),
            commands: Vec::new(),
        }
    }

    /// Returns a reference to the shared world.
    pub fn world(&self) -> &Arc<World> {
        &self.world
    }

    /// Registers a plugin's event handlers and commands.
    pub fn register(&mut self, plugin: impl Plugin) {
        let mut registrar = PluginRegistrar::new(&mut self.bus, &mut self.commands);
        plugin.on_enable(&mut registrar);
    }

    /// Creates a default server context for "Steve" with entity ID 1.
    pub fn context(&self) -> ServerContext {
        ServerContext::new(
            Arc::clone(&self.world),
            Uuid::default(),
            1,
            "Steve".into(),
            0.0,
            0.0,
        )
    }

    /// Creates a server context with custom player identity.
    pub fn context_for(&self, uuid: Uuid, entity_id: i32, username: &str) -> ServerContext {
        ServerContext::new(
            Arc::clone(&self.world),
            uuid,
            entity_id,
            username.to_string(),
            0.0,
            0.0,
        )
    }

    /// Dispatches an event and returns the queued responses.
    ///
    /// Creates a default context, dispatches the event through the bus,
    /// and drains the response queue.
    pub fn dispatch(&self, event: &mut dyn Event) -> Vec<Response> {
        let ctx = self.context();
        self.bus.dispatch_dyn(event, &ctx);
        ctx.drain_responses()
    }

    /// Dispatches an event with a specific context and returns responses.
    pub fn dispatch_with(&self, event: &mut dyn Event, ctx: &ServerContext) -> Vec<Response> {
        self.bus.dispatch_dyn(event, ctx);
        ctx.drain_responses()
    }

    /// Returns a reference to the collected command entries.
    pub fn commands(&self) -> &[basalt_api::CommandEntry] {
        &self.commands
    }
}

impl Default for PluginTestHarness {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_creates_world() {
        let harness = PluginTestHarness::new();
        assert!(harness.world().chunk_count() == 0);
    }

    #[test]
    fn harness_default() {
        let harness = PluginTestHarness::default();
        assert!(harness.commands().is_empty());
    }

    #[test]
    fn context_has_default_identity() {
        let harness = PluginTestHarness::new();
        let ctx = harness.context();
        use basalt_api::Context;
        assert_eq!(ctx.player_username(), "Steve");
        assert_eq!(ctx.player_entity_id(), 1);
    }
}
