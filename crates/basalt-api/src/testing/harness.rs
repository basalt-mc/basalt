//! Plugin and system test harnesses.
//!
//! Provides [`PluginTestHarness`] for event-based plugin testing and
//! [`SystemTestContext`] for ECS system plugin testing.

use std::sync::Arc;

use crate::components::Rotation;
use crate::context::ServerContext;
use crate::events::{BusKind, EventRouting};
use crate::player::PlayerInfo;
use crate::plugin::PluginRegistrar;
use crate::{Event, EventBus, Plugin, Response, Stage};
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
    commands: Vec<crate::plugin::CommandEntry>,
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
        handler: impl Fn(&mut E, &dyn crate::context::Context) + Send + Sync + 'static,
    ) where
        E: Event + EventRouting + 'static,
    {
        let wrapper = move |event: &mut E, ctx: &ServerContext| {
            handler(event, ctx as &dyn crate::context::Context);
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
                position: crate::components::Position {
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
                position: crate::components::Position {
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

    /// Executes a command by name and returns a [`DispatchResult`].
    ///
    /// Looks up the command in registered commands, parses arguments,
    /// and calls the handler. Returns empty if the command is not found.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let responses = harness.dispatch_command("tp 10 64 -5");
    /// ```
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
                crate::command::parse_command_args(args, &entry.args, &entry.variants)
        {
            (entry.handler)(&parsed, &ctx);
        }
        DispatchResult {
            responses: ctx.drain_responses(),
        }
    }

    /// Returns a reference to the collected command entries.
    pub fn commands(&self) -> &[crate::plugin::CommandEntry] {
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
                Response::Broadcast(crate::broadcast::BroadcastMessage::Chat { .. })
            )
        })
    }

    /// Returns true if any response is a block change broadcast.
    pub fn has_block_change_broadcast(&self) -> bool {
        self.responses.iter().any(|r| {
            matches!(
                r,
                Response::Broadcast(crate::broadcast::BroadcastMessage::BlockChanged { .. })
            )
        })
    }

    /// Returns true if any response is an entity moved broadcast.
    pub fn has_entity_moved_broadcast(&self) -> bool {
        self.responses.iter().any(|r| {
            matches!(
                r,
                Response::Broadcast(crate::broadcast::BroadcastMessage::EntityMoved { .. })
            )
        })
    }

    /// Returns true if any response is a player joined broadcast.
    pub fn has_player_joined_broadcast(&self) -> bool {
        self.responses.iter().any(|r| {
            matches!(
                r,
                Response::Broadcast(crate::broadcast::BroadcastMessage::PlayerJoined { .. })
            )
        })
    }

    /// Returns true if any response is a player left broadcast.
    pub fn has_player_left_broadcast(&self) -> bool {
        self.responses.iter().any(|r| {
            matches!(
                r,
                Response::Broadcast(crate::broadcast::BroadcastMessage::PlayerLeft { .. })
            )
        })
    }

    /// Returns true if any response streams chunks to the given position.
    pub fn has_stream_chunks(&self, x: i32, z: i32) -> bool {
        self.responses.iter().any(|r| {
            matches!(
                r,
                Response::StreamChunks(crate::components::ChunkPosition { x: cx, z: cz })
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
/// Contains a minimal entity-component store and a [`World`],
/// implementing [`SystemContext`](crate::system::SystemContext) so
/// system runners can be tested without a full server or `basalt-ecs`.
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
    /// Per-entity component storage keyed by `(EntityId, TypeId)`.
    components: std::collections::HashMap<
        (crate::components::EntityId, std::any::TypeId),
        Box<dyn std::any::Any + Send + Sync>,
    >,
    /// Set of live entity IDs.
    entities: std::collections::HashSet<crate::components::EntityId>,
    /// Counter for generating unique entity IDs.
    next_id: crate::components::EntityId,
    /// Shared world for block/collision queries.
    world: Arc<World>,
    /// Unlimited budget for test systems.
    budget: crate::budget::TickBudget,
}

impl SystemTestContext {
    /// Creates a new context with a flat world.
    pub fn new() -> Self {
        Self {
            components: std::collections::HashMap::new(),
            entities: std::collections::HashSet::new(),
            next_id: 0,
            world: Arc::new(World::flat()),
            budget: crate::budget::TickBudget::unlimited(),
        }
    }

    /// Creates a new context with a custom world.
    pub fn with_world(world: Arc<World>) -> Self {
        Self {
            components: std::collections::HashMap::new(),
            entities: std::collections::HashSet::new(),
            next_id: 0,
            world,
            budget: crate::budget::TickBudget::unlimited(),
        }
    }
}

impl Default for SystemTestContext {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::system::SystemContext for SystemTestContext {
    fn get_block(&self, x: i32, y: i32, z: i32) -> u16 {
        self.world.get_block(x, y, z)
    }

    fn set_block(&self, x: i32, y: i32, z: i32, state: u16) {
        self.world.set_block(x, y, z, state);
    }

    fn get_block_entity(
        &self,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<basalt_world::block_entity::BlockEntity> {
        self.world.get_block_entity(x, y, z).map(|r| r.clone())
    }

    fn set_block_entity(
        &self,
        x: i32,
        y: i32,
        z: i32,
        entity: basalt_world::block_entity::BlockEntity,
    ) {
        self.world.set_block_entity(x, y, z, entity);
    }

    fn mark_chunk_dirty(&self, cx: i32, cz: i32) {
        self.world.mark_chunk_dirty(cx, cz);
    }

    fn check_overlap(&self, aabb: &crate::world::collision::Aabb) -> bool {
        crate::world::collision::check_overlap(&self.world, aabb)
    }

    fn ray_cast(
        &self,
        origin: (f64, f64, f64),
        direction: (f64, f64, f64),
        max_distance: f64,
    ) -> Option<crate::world::collision::RayHit> {
        crate::world::collision::ray_cast(&self.world, origin, direction, max_distance)
    }

    fn resolve_movement(
        &self,
        aabb: &crate::world::collision::Aabb,
        dx: f64,
        dy: f64,
        dz: f64,
    ) -> (f64, f64, f64) {
        crate::world::collision::resolve_movement(&self.world, aabb, dx, dy, dz)
    }

    fn spawn(&mut self) -> crate::components::EntityId {
        let id = self.next_id;
        self.next_id += 1;
        self.entities.insert(id);
        id
    }

    fn despawn(&mut self, entity: crate::components::EntityId) {
        self.entities.remove(&entity);
        self.components.retain(|&(eid, _), _| eid != entity);
    }

    fn set_component(
        &mut self,
        entity: crate::components::EntityId,
        type_id: std::any::TypeId,
        component: Box<dyn std::any::Any + Send + Sync>,
    ) {
        self.components.insert((entity, type_id), component);
    }

    fn entities_with(&self, type_id: std::any::TypeId) -> Vec<crate::components::EntityId> {
        self.entities
            .iter()
            .filter(|&&eid| self.components.contains_key(&(eid, type_id)))
            .copied()
            .collect()
    }

    fn get_component(
        &self,
        entity: crate::components::EntityId,
        type_id: std::any::TypeId,
    ) -> Option<&dyn std::any::Any> {
        self.components
            .get(&(entity, type_id))
            .map(|b| b.as_ref() as &dyn std::any::Any)
    }

    fn get_component_mut(
        &mut self,
        entity: crate::components::EntityId,
        type_id: std::any::TypeId,
    ) -> Option<&mut dyn std::any::Any> {
        self.components
            .get_mut(&(entity, type_id))
            .map(|b| b.as_mut() as &mut dyn std::any::Any)
    }

    fn budget(&self) -> &crate::budget::TickBudget {
        &self.budget
    }
}
