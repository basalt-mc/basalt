//! Shared test helpers for Basalt plugin and server tests.
//!
//! Provides [`PluginTestHarness`] to eliminate the duplicated test
//! scaffolding (world creation, event bus, plugin registration, dispatch)
//! that appears in every plugin's test module.
//!
//! # Example
//!
//! ```ignore
//! let mut harness = PluginTestHarness::new();
//! harness.register(MyPlugin);
//!
//! // Dispatch an event
//! let mut event = BlockBrokenEvent { x: 5, y: 64, z: 3, ... };
//! let responses = harness.dispatch(&mut event);
//! assert_eq!(responses.len(), 2);
//!
//! // Execute a command
//! let responses = harness.dispatch_command("tp 10 64 -5");
//! assert!(matches!(responses[0], Response::SendPosition { .. }));
//! ```

use std::sync::Arc;

use basalt_api::context::ServerContext;
use basalt_api::plugin::PluginRegistrar;
use basalt_api::{EventBus, Plugin, Response};
use basalt_events::{Event, EventRouting, Stage};
use basalt_types::Uuid;
use basalt_world::World;

/// Test harness for plugin development.
///
/// Provides a simple API for testing plugins without importing
/// internal types. Handles world creation, event bus setup, plugin
/// registration, event dispatch, and command execution.
pub struct PluginTestHarness {
    /// Shared world instance for the test.
    world: Arc<World>,
    /// Event bus for instant events (chat, commands).
    instant_bus: EventBus,
    /// Event bus for game events (blocks, movement, lifecycle).
    game_bus: EventBus,
    /// Collected command entries.
    commands: Vec<basalt_api::CommandEntry>,
}

impl PluginTestHarness {
    /// Creates a new test harness with a memory-only world (seed 42).
    pub fn new() -> Self {
        Self {
            world: Arc::new(World::new_memory(42)),
            instant_bus: EventBus::new(),
            game_bus: EventBus::new(),
            commands: Vec::new(),
        }
    }

    /// Creates a new test harness with the given world.
    pub fn with_world(world: Arc<World>) -> Self {
        Self {
            world,
            instant_bus: EventBus::new(),
            game_bus: EventBus::new(),
            commands: Vec::new(),
        }
    }

    /// Returns a reference to the shared world.
    pub fn world(&self) -> &Arc<World> {
        &self.world
    }

    /// Registers a plugin's event handlers and commands.
    pub fn register(&mut self, plugin: impl Plugin) {
        let mut systems = Vec::new();
        let mut components = Vec::new();
        let mut registrar = PluginRegistrar::new(
            &mut self.instant_bus,
            &mut self.game_bus,
            &mut self.commands,
            &mut systems,
            &mut components,
            Arc::clone(&self.world),
        );
        plugin.on_enable(&mut registrar);
    }

    /// Registers an ad-hoc event handler (for testing cancellation, custom logic, etc.).
    ///
    /// The handler is routed to the correct bus based on `E::BUS`.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Register a Validate handler that cancels the event
    /// harness.on::<BlockBrokenEvent>(Stage::Validate, 0, |event, _ctx| {
    ///     event.cancel();
    /// });
    /// ```
    pub fn on<E>(
        &mut self,
        stage: Stage,
        priority: i32,
        handler: impl Fn(&mut E, &ServerContext) + Send + Sync + 'static,
    ) where
        E: Event + EventRouting + 'static,
    {
        match E::BUS {
            basalt_events::BusKind::Instant => {
                self.instant_bus
                    .on::<E, ServerContext>(stage, priority, handler);
            }
            basalt_events::BusKind::Game => {
                self.game_bus
                    .on::<E, ServerContext>(stage, priority, handler);
            }
        }
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
    pub fn dispatch(&self, event: &mut dyn Event) -> Vec<Response> {
        let ctx = self.context();
        self.dispatch_routed(event, &ctx);
        let responses = ctx.drain_responses();
        for response in &responses {
            if let Response::PersistChunk { cx, cz } = response {
                self.world.persist_chunk(*cx, *cz);
            }
        }
        responses
    }

    /// Dispatches an event with a specific context and returns responses.
    pub fn dispatch_with(&self, event: &mut dyn Event, ctx: &ServerContext) -> Vec<Response> {
        self.dispatch_routed(event, ctx);
        ctx.drain_responses()
    }

    /// Executes a command by name and returns the responses.
    ///
    /// Looks up the command in registered commands, parses arguments,
    /// and calls the handler. Returns empty if the command is not found.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let responses = harness.dispatch_command("tp 10 64 -5");
    /// ```
    pub fn dispatch_command(&self, command: &str) -> Vec<Response> {
        let ctx = self.context();
        ctx.set_command_list(
            self.commands
                .iter()
                .map(|c| (c.name.clone(), c.description.clone()))
                .collect(),
        );

        let parts: Vec<&str> = command.splitn(2, ' ').collect();
        let name = parts[0];
        let args = parts.get(1).copied().unwrap_or("");

        if let Some(entry) = self.commands.iter().find(|c| c.name == name)
            && let Ok(parsed) =
                basalt_command::parse_command_args(args, &entry.args, &entry.variants)
        {
            (entry.handler)(&parsed, &ctx);
        }
        ctx.drain_responses()
    }

    /// Returns a reference to the collected command entries.
    pub fn commands(&self) -> &[basalt_api::CommandEntry] {
        &self.commands
    }

    /// Routes a type-erased event to the correct bus.
    fn dispatch_routed(&self, event: &mut dyn Event, ctx: &ServerContext) {
        match event.bus_kind() {
            basalt_events::BusKind::Instant => self.instant_bus.dispatch_dyn(event, ctx),
            basalt_events::BusKind::Game => self.game_bus.dispatch_dyn(event, ctx),
        }
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
