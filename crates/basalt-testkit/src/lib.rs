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
//! let mut event = BlockBrokenEvent { position: BlockPosition { x: 5, y: 64, z: 3 }, ... };
//! let result = harness.dispatch(&mut event);
//! assert_eq!(result.len(), 2);
//! assert!(result.has_block_ack());
//!
//! // Execute a command
//! let result = harness.dispatch_command("tp 10 64 -5");
//! assert!(result.has_teleport());
//! ```

use std::sync::Arc;

use basalt_api::components::Rotation;
use basalt_api::context::ServerContext;
use basalt_api::events::{BusKind, EventRouting};
use basalt_api::player::PlayerInfo;
use basalt_api::plugin::PluginRegistrar;
use basalt_api::{Event, Stage};
use basalt_api::{EventBus, Plugin, Response};
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
    commands: Vec<basalt_api::plugin::CommandEntry>,
    /// Recipe registry for plugin customisation.
    recipes: basalt_recipes::RecipeRegistry,
}

impl PluginTestHarness {
    /// Creates a new test harness with a memory-only world (seed 42).
    pub fn new() -> Self {
        Self {
            world: Arc::new(World::new_memory(42)),
            instant_bus: EventBus::new(),
            game_bus: EventBus::new(),
            commands: Vec::new(),
            recipes: basalt_recipes::RecipeRegistry::empty(),
        }
    }

    /// Creates a new test harness with the given world.
    pub fn with_world(world: Arc<World>) -> Self {
        Self {
            world,
            instant_bus: EventBus::new(),
            game_bus: EventBus::new(),
            commands: Vec::new(),
            recipes: basalt_recipes::RecipeRegistry::empty(),
        }
    }

    /// Returns a reference to the shared world.
    pub fn world(&self) -> &Arc<World> {
        &self.world
    }

    /// Registers a plugin's event handlers and commands.
    ///
    /// Builds a stub `ServerContext` so that any registry-lifecycle
    /// events fired during `on_enable` (e.g. `RecipeRegisteredEvent`)
    /// dispatch through the harness in the same way the production
    /// server does.
    pub fn register(&mut self, plugin: impl Plugin) {
        let mut systems = Vec::new();
        let bootstrap_ctx = ServerContext::new(Arc::clone(&self.world), PlayerInfo::stub());
        let mut registrar = PluginRegistrar::new(
            &mut self.instant_bus,
            &mut self.game_bus,
            &mut self.commands,
            &mut systems,
            Arc::clone(&self.world),
            &mut self.recipes,
            &bootstrap_ctx,
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
        handler: impl Fn(&mut E, &dyn basalt_api::context::Context) + Send + Sync + 'static,
    ) where
        E: Event + EventRouting + 'static,
    {
        let wrapper = move |event: &mut E, ctx: &ServerContext| {
            handler(event, ctx as &dyn basalt_api::context::Context);
        };
        match E::BUS {
            BusKind::Instant => {
                self.instant_bus
                    .on::<E, ServerContext>(stage, priority, wrapper);
            }
            BusKind::Game => {
                self.game_bus
                    .on::<E, ServerContext>(stage, priority, wrapper);
            }
        }
    }

    /// Creates a default server context for "Steve" with entity ID 1.
    pub fn context(&self) -> ServerContext {
        ServerContext::new(
            Arc::clone(&self.world),
            PlayerInfo {
                uuid: Uuid::default(),
                entity_id: 1,
                username: "Steve".into(),
                rotation: Rotation {
                    yaw: 0.0,
                    pitch: 0.0,
                },
                position: basalt_api::components::Position {
                    x: 0.0,
                    y: 64.0,
                    z: 0.0,
                },
            },
        )
    }

    /// Creates a server context with custom player identity.
    pub fn context_for(&self, uuid: Uuid, entity_id: i32, username: &str) -> ServerContext {
        ServerContext::new(
            Arc::clone(&self.world),
            PlayerInfo {
                uuid,
                entity_id,
                username: username.to_string(),
                rotation: Rotation {
                    yaw: 0.0,
                    pitch: 0.0,
                },
                position: basalt_api::components::Position {
                    x: 0.0,
                    y: 64.0,
                    z: 0.0,
                },
            },
        )
    }

    /// Dispatches an event and returns a [`DispatchResult`] for assertions.
    pub fn dispatch(&self, event: &mut dyn Event) -> DispatchResult {
        let ctx = self.context();
        self.dispatch_routed(event, &ctx);
        let responses = ctx.drain_responses();
        for response in &responses {
            if let Response::PersistChunk(chunk) = response {
                self.world.persist_chunk(chunk.x, chunk.z);
            }
        }
        DispatchResult { responses }
    }

    /// Dispatches an event with a specific context and returns a [`DispatchResult`].
    pub fn dispatch_with(&self, event: &mut dyn Event, ctx: &ServerContext) -> DispatchResult {
        self.dispatch_routed(event, ctx);
        DispatchResult {
            responses: ctx.drain_responses(),
        }
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
    /// Executes a command by name and returns a [`DispatchResult`].
    pub fn dispatch_command(&self, command: &str) -> DispatchResult {
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
                basalt_api::command::parse_command_args(args, &entry.args, &entry.variants)
        {
            (entry.handler)(&parsed, &ctx);
        }
        DispatchResult {
            responses: ctx.drain_responses(),
        }
    }

    /// Returns a reference to the collected command entries.
    pub fn commands(&self) -> &[basalt_api::plugin::CommandEntry] {
        &self.commands
    }

    /// Routes a type-erased event to the correct bus.
    fn dispatch_routed(&self, event: &mut dyn Event, ctx: &ServerContext) {
        match event.bus_kind() {
            BusKind::Instant => self.instant_bus.dispatch_dyn(event, ctx),
            BusKind::Game => self.game_bus.dispatch_dyn(event, ctx),
        }
    }
}

impl Default for PluginTestHarness {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of dispatching an event through the test harness.
///
/// Wraps the internal response queue and provides high-level assertion
/// methods. Plugin tests use this instead of inspecting `Response`
/// variants directly.
pub struct DispatchResult {
    responses: Vec<Response>,
}

impl DispatchResult {
    /// Returns the number of queued responses.
    pub fn len(&self) -> usize {
        self.responses.len()
    }

    /// Returns true if no responses were queued.
    pub fn is_empty(&self) -> bool {
        self.responses.is_empty()
    }

    /// Returns true if any response is a block acknowledgement.
    pub fn has_block_ack(&self) -> bool {
        self.responses
            .iter()
            .any(|r| matches!(r, Response::SendBlockAck { .. }))
    }

    /// Returns true if any response is a block ack with the given sequence.
    pub fn has_block_ack_seq(&self, seq: i32) -> bool {
        self.responses
            .iter()
            .any(|r| matches!(r, Response::SendBlockAck { sequence } if *sequence == seq))
    }

    /// Returns true if any response is a system chat message.
    pub fn has_system_chat(&self) -> bool {
        self.responses
            .iter()
            .any(|r| matches!(r, Response::SendSystemChat { .. }))
    }

    /// Returns true if any response is a teleport.
    pub fn has_teleport(&self) -> bool {
        self.responses
            .iter()
            .any(|r| matches!(r, Response::SendPosition { .. }))
    }

    /// Returns true if any response is a game state change.
    pub fn has_game_state_change(&self) -> bool {
        self.responses
            .iter()
            .any(|r| matches!(r, Response::SendGameStateChange { .. }))
    }

    /// Returns true if any response is a chat broadcast.
    pub fn has_chat_broadcast(&self) -> bool {
        self.responses.iter().any(|r| {
            matches!(
                r,
                Response::Broadcast(basalt_api::broadcast::BroadcastMessage::Chat { .. })
            )
        })
    }

    /// Returns true if any response is a block change broadcast.
    pub fn has_block_change_broadcast(&self) -> bool {
        self.responses.iter().any(|r| {
            matches!(
                r,
                Response::Broadcast(basalt_api::broadcast::BroadcastMessage::BlockChanged { .. })
            )
        })
    }

    /// Returns true if any response is an entity moved broadcast.
    pub fn has_entity_moved_broadcast(&self) -> bool {
        self.responses.iter().any(|r| {
            matches!(
                r,
                Response::Broadcast(basalt_api::broadcast::BroadcastMessage::EntityMoved { .. })
            )
        })
    }

    /// Returns true if any response is a player joined broadcast.
    pub fn has_player_joined_broadcast(&self) -> bool {
        self.responses.iter().any(|r| {
            matches!(
                r,
                Response::Broadcast(basalt_api::broadcast::BroadcastMessage::PlayerJoined { .. })
            )
        })
    }

    /// Returns true if any response is a player left broadcast.
    pub fn has_player_left_broadcast(&self) -> bool {
        self.responses.iter().any(|r| {
            matches!(
                r,
                Response::Broadcast(basalt_api::broadcast::BroadcastMessage::PlayerLeft { .. })
            )
        })
    }

    /// Returns true if any response streams chunks to the given position.
    pub fn has_stream_chunks(&self, x: i32, z: i32) -> bool {
        self.responses.iter().any(|r| {
            matches!(
                r,
                Response::StreamChunks(basalt_api::components::ChunkPosition { x: cx, z: cz })
                if *cx == x && *cz == z
            )
        })
    }

    /// Returns true if any response spawns a dropped item with the given ID and count.
    pub fn has_spawn_dropped_item(&self, item_id: i32, count: i32) -> bool {
        self.responses.iter().any(|r| {
            matches!(
                r,
                Response::SpawnDroppedItem { item_id: id, count: c, .. }
                if *id == item_id && *c == count
            )
        })
    }

    /// Returns true if any response spawns any dropped item.
    pub fn has_any_spawn_dropped_item(&self) -> bool {
        self.responses
            .iter()
            .any(|r| matches!(r, Response::SpawnDroppedItem { .. }))
    }

    /// Returns true if any response broadcasts a `BlockAction` packet.
    pub fn has_broadcast_block_action(&self) -> bool {
        self.responses
            .iter()
            .any(|r| matches!(r, Response::BroadcastBlockAction { .. }))
    }

    /// Returns true if any response notifies co-viewers of a slot change.
    pub fn has_notify_viewers(&self) -> bool {
        self.responses
            .iter()
            .any(|r| matches!(r, Response::NotifyContainerViewers { .. }))
    }

    /// Returns true if any response queues a block-entity destroy.
    pub fn has_destroy_block_entity(&self) -> bool {
        self.responses
            .iter()
            .any(|r| matches!(r, Response::DestroyBlockEntity { .. }))
    }

    /// Returns true if any response unlocks the given recipe id.
    pub fn has_unlock_recipe(&self, id: &basalt_recipes::RecipeId) -> bool {
        self.responses.iter().any(|r| {
            matches!(
                r,
                Response::UnlockRecipe { recipe_id, .. } if recipe_id == id
            )
        })
    }

    /// Returns true if any response locks the given recipe id.
    pub fn has_lock_recipe(&self, id: &basalt_recipes::RecipeId) -> bool {
        self.responses.iter().any(|r| {
            matches!(
                r,
                Response::LockRecipe { recipe_id } if recipe_id == id
            )
        })
    }
}

/// Test context for system plugins.
///
/// Pairs an [`Ecs`](basalt_ecs::Ecs) with a [`World`] and implements
/// [`SystemContext`](basalt_api::system::SystemContext) so system runners can
/// be tested without a full server.
///
/// # Example
///
/// ```ignore
/// let mut ctx = SystemTestContext::new();
/// let e = ctx.spawn();
/// ctx.set::<Position>(e, Position { x: 0.0, y: 64.0, z: 0.0 });
/// ctx.set::<Velocity>(e, Velocity { dx: 0.0, dy: 0.0, dz: 0.0 });
/// physics_tick(&mut ctx);
/// ```
pub struct SystemTestContext {
    /// The ECS instance.
    pub ecs: basalt_ecs::Ecs,
    /// Shared world for block/collision queries.
    world: Arc<World>,
    /// Unlimited budget for test systems.
    budget: basalt_api::budget::TickBudget,
}

impl SystemTestContext {
    /// Creates a new context with a flat world.
    pub fn new() -> Self {
        Self {
            ecs: basalt_ecs::Ecs::new(),
            world: Arc::new(World::flat()),
            budget: basalt_api::budget::TickBudget::unlimited(),
        }
    }

    /// Creates a new context with a custom world.
    pub fn with_world(world: Arc<World>) -> Self {
        Self {
            ecs: basalt_ecs::Ecs::new(),
            world,
            budget: basalt_api::budget::TickBudget::unlimited(),
        }
    }
}

impl Default for SystemTestContext {
    fn default() -> Self {
        Self::new()
    }
}

impl basalt_api::system::SystemContext for SystemTestContext {
    fn world(&self) -> &World {
        &self.world
    }

    fn spawn(&mut self) -> basalt_api::components::EntityId {
        self.ecs.spawn()
    }

    fn despawn(&mut self, entity: basalt_api::components::EntityId) {
        self.ecs.despawn(entity);
    }

    fn set_component(
        &mut self,
        entity: basalt_api::components::EntityId,
        type_id: std::any::TypeId,
        component: Box<dyn std::any::Any + Send + Sync>,
    ) {
        self.ecs.set_component(entity, type_id, component);
    }

    fn entities_with(&self, type_id: std::any::TypeId) -> Vec<basalt_api::components::EntityId> {
        self.ecs.entities_with(type_id)
    }

    fn get_component(
        &self,
        entity: basalt_api::components::EntityId,
        type_id: std::any::TypeId,
    ) -> Option<&dyn std::any::Any> {
        self.ecs.get_component(entity, type_id)
    }

    fn get_component_mut(
        &mut self,
        entity: basalt_api::components::EntityId,
        type_id: std::any::TypeId,
    ) -> Option<&mut dyn std::any::Any> {
        self.ecs.get_component_mut(entity, type_id)
    }

    fn budget(&self) -> &basalt_api::budget::TickBudget {
        &self.budget
    }
}
