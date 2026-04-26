//! Plugin and system test harnesses.
//!
//! Provides [`PluginTestHarness`] for event-based plugin testing and
//! [`SystemTestContext`] for ECS system plugin testing.

use std::cell::RefCell;
use std::sync::Arc;

use crate::broadcast::BroadcastMessage;
use crate::components::{KnownRecipes, Rotation};
use crate::context::{
    ChatContext, ContainerContext, Context, EntityContext, PlayerContext, RecipeContext, Response,
    ResponseQueue, UnlockReason, WorldContext,
};
use crate::events::{BusKind, EventRouting};
use crate::gamemode::Gamemode;
use crate::logger::PluginLogger;
use crate::player::PlayerInfo;
use crate::plugin::PluginRegistrar;
use crate::testing::noop::NoopContext;
use crate::world::collision::{Aabb, RayHit};
use crate::world::handle::WorldHandle;
use crate::{Event, EventBus, Plugin, Stage};
use basalt_recipes::RecipeId;
use basalt_types::{Slot, TextComponent, Uuid};

use super::mock_world::MockWorld;

// ── HarnessContext ──────────────────────────────────────────────────

/// Lightweight context for the test harness.
///
/// Mirrors the production `ServerContext` (in basalt-server) but lives
/// in basalt-api so the harness does not depend on basalt-server. It
/// implements [`Context`] and all sub-context traits with real
/// response queueing and world delegation, which is all the harness
/// needs for event dispatch assertions.
struct HarnessContext {
    /// Shared world reference for block access and chunk persistence.
    world: Arc<dyn WorldHandle + Send + Sync>,
    /// Queue for deferred async responses.
    responses: ResponseQueue,
    /// Identity and state of the player who triggered this action.
    player: PlayerInfo,
    /// Snapshot of the player's known recipes.
    known_recipes: KnownRecipes,
    /// Name of the plugin currently being dispatched.
    plugin_name: RefCell<String>,
    /// Registered command list (name, description) for /help.
    command_list: RefCell<Vec<(String, String)>>,
}

impl HarnessContext {
    /// Creates a new harness context for a single event dispatch.
    fn new(world: Arc<dyn WorldHandle + Send + Sync>, player: PlayerInfo) -> Self {
        Self {
            world,
            responses: ResponseQueue::new(),
            player,
            known_recipes: KnownRecipes::default(),
            plugin_name: RefCell::new(String::new()),
            command_list: RefCell::new(Vec::new()),
        }
    }

    /// Sets the registered command list for /help.
    fn set_command_list(&self, commands: Vec<(String, String)>) {
        *self.command_list.borrow_mut() = commands;
    }

    /// Drains all queued responses.
    fn drain_responses(&self) -> Vec<Response> {
        self.responses.drain()
    }
}

impl PlayerContext for HarnessContext {
    fn uuid(&self) -> Uuid {
        self.player.uuid
    }
    fn entity_id(&self) -> i32 {
        self.player.entity_id
    }
    fn username(&self) -> &str {
        &self.player.username
    }
    fn yaw(&self) -> f32 {
        self.player.rotation.yaw
    }
    fn pitch(&self) -> f32 {
        self.player.rotation.pitch
    }
    fn position(&self) -> (f64, f64, f64) {
        let p = self.player.position;
        (p.x, p.y, p.z)
    }
    fn teleport(&self, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) {
        use std::sync::atomic::{AtomicI32, Ordering};
        static TELEPORT_COUNTER: AtomicI32 = AtomicI32::new(1);
        let teleport_id = TELEPORT_COUNTER.fetch_add(1, Ordering::Relaxed);
        self.responses.push(Response::SendPosition {
            teleport_id,
            position: crate::components::Position { x, y, z },
            rotation: Rotation { yaw, pitch },
        });
    }
    fn set_gamemode(&self, mode: Gamemode) {
        self.responses.push(Response::SendGameStateChange {
            reason: 3,
            value: mode.id() as f32,
        });
    }
    fn registered_commands(&self) -> Vec<(String, String)> {
        self.command_list.borrow().clone()
    }
}

impl ChatContext for HarnessContext {
    fn send(&self, text: &str) {
        let component = TextComponent::text(text);
        self.send_component(&component);
    }
    fn send_component(&self, component: &TextComponent) {
        self.responses.push(Response::SendSystemChat {
            content: component.to_nbt(),
            action_bar: false,
        });
    }
    fn action_bar(&self, text: &str) {
        let component = TextComponent::text(text);
        self.responses.push(Response::SendSystemChat {
            content: component.to_nbt(),
            action_bar: true,
        });
    }
    fn broadcast(&self, text: &str) {
        let component = TextComponent::text(text);
        self.broadcast_component(&component);
    }
    fn broadcast_component(&self, component: &TextComponent) {
        self.responses
            .push(Response::Broadcast(BroadcastMessage::Chat {
                content: component.to_nbt(),
            }));
    }
}

impl WorldHandle for HarnessContext {
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
    ) -> Option<crate::world::block_entity::BlockEntity> {
        self.world.get_block_entity(x, y, z)
    }
    fn set_block_entity(
        &self,
        x: i32,
        y: i32,
        z: i32,
        entity: crate::world::block_entity::BlockEntity,
    ) {
        self.world.set_block_entity(x, y, z, entity);
    }
    fn mark_chunk_dirty(&self, cx: i32, cz: i32) {
        self.world.mark_chunk_dirty(cx, cz);
    }
    fn persist_chunk(&self, cx: i32, cz: i32) {
        self.world.persist_chunk(cx, cz);
    }
    fn dirty_chunks(&self) -> Vec<(i32, i32)> {
        self.world.dirty_chunks()
    }
    fn check_overlap(&self, aabb: &Aabb) -> bool {
        self.world.check_overlap(aabb)
    }
    fn ray_cast(
        &self,
        origin: (f64, f64, f64),
        direction: (f64, f64, f64),
        max_distance: f64,
    ) -> Option<RayHit> {
        self.world.ray_cast(origin, direction, max_distance)
    }
    fn resolve_movement(&self, aabb: &Aabb, dx: f64, dy: f64, dz: f64) -> (f64, f64, f64) {
        self.world.resolve_movement(aabb, dx, dy, dz)
    }
}

impl WorldContext for HarnessContext {
    fn send_block_ack(&self, sequence: i32) {
        self.responses.push(Response::SendBlockAck { sequence });
    }
    fn stream_chunks(&self, cx: i32, cz: i32) {
        self.responses
            .push(Response::StreamChunks(crate::components::ChunkPosition {
                x: cx,
                z: cz,
            }));
    }
    fn queue_persist_chunk(&self, cx: i32, cz: i32) {
        self.responses
            .push(Response::PersistChunk(crate::components::ChunkPosition {
                x: cx,
                z: cz,
            }));
    }
    fn destroy_block_entity(&self, x: i32, y: i32, z: i32) {
        self.responses.push(Response::DestroyBlockEntity {
            position: crate::components::BlockPosition { x, y, z },
        });
    }
}

impl EntityContext for HarnessContext {
    fn spawn_dropped_item(&self, x: i32, y: i32, z: i32, item_id: i32, count: i32) {
        self.responses.push(Response::SpawnDroppedItem {
            position: crate::components::BlockPosition { x, y, z },
            item_id,
            count,
        });
    }
    fn broadcast_block_change(&self, x: i32, y: i32, z: i32, block_state: i32) {
        self.responses
            .push(Response::Broadcast(BroadcastMessage::BlockChanged {
                x,
                y,
                z,
                block_state,
            }));
    }
    #[allow(clippy::too_many_arguments)]
    fn broadcast_entity_moved(
        &self,
        entity_id: i32,
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
        on_ground: bool,
    ) {
        self.responses
            .push(Response::Broadcast(BroadcastMessage::EntityMoved {
                entity_id,
                x,
                y,
                z,
                yaw,
                pitch,
                on_ground,
            }));
    }
    fn broadcast_player_joined(&self) {
        self.responses
            .push(Response::Broadcast(BroadcastMessage::PlayerJoined {
                info: crate::broadcast::PlayerSnapshot {
                    username: self.player.username.clone(),
                    uuid: self.player.uuid,
                    entity_id: self.player.entity_id,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    yaw: self.player.rotation.yaw,
                    pitch: self.player.rotation.pitch,
                    skin_properties: Vec::new(),
                },
            }));
    }
    fn broadcast_player_left(&self) {
        self.responses
            .push(Response::Broadcast(BroadcastMessage::PlayerLeft {
                uuid: self.player.uuid,
                entity_id: self.player.entity_id,
                username: self.player.username.clone(),
            }));
    }
    fn broadcast_raw(&self, msg: BroadcastMessage) {
        self.responses.push(Response::Broadcast(msg));
    }
    fn broadcast_block_action(
        &self,
        x: i32,
        y: i32,
        z: i32,
        action_id: u8,
        action_param: u8,
        block_id: i32,
    ) {
        self.responses.push(Response::BroadcastBlockAction {
            position: crate::components::BlockPosition { x, y, z },
            action_id,
            action_param,
            block_id,
        });
    }
}

impl ContainerContext for HarnessContext {
    fn open_chest(&self, x: i32, y: i32, z: i32) {
        self.responses
            .push(Response::OpenChest(crate::components::BlockPosition {
                x,
                y,
                z,
            }));
    }
    fn open_crafting_table(&self, x: i32, y: i32, z: i32) {
        self.responses.push(Response::OpenCraftingTable {
            position: crate::components::BlockPosition { x, y, z },
        });
    }
    fn open(&self, container: &crate::container::Container) {
        self.responses
            .push(Response::OpenContainer(container.clone()));
    }
    fn notify_viewers(&self, x: i32, y: i32, z: i32, slot_index: i16, item: Slot) {
        self.responses.push(Response::NotifyContainerViewers {
            position: crate::components::BlockPosition { x, y, z },
            slot_index,
            item,
        });
    }
}

impl RecipeContext for HarnessContext {
    fn unlock(&self, id: &RecipeId, reason: UnlockReason) {
        self.responses.push(Response::UnlockRecipe {
            recipe_id: id.clone(),
            reason,
        });
    }
    fn lock(&self, id: &RecipeId) {
        self.responses.push(Response::LockRecipe {
            recipe_id: id.clone(),
        });
    }
    fn has(&self, id: &RecipeId) -> bool {
        self.known_recipes.has(id)
    }
    fn unlocked(&self) -> Vec<RecipeId> {
        self.known_recipes
            .iter()
            .map(|(id, _)| id.clone())
            .collect()
    }
}

impl Context for HarnessContext {
    fn logger(&self) -> PluginLogger {
        PluginLogger::new(&self.plugin_name.borrow())
    }
    fn player(&self) -> &dyn PlayerContext {
        self
    }
    fn chat(&self) -> &dyn ChatContext {
        self
    }
    fn world_ctx(&self) -> &dyn WorldContext {
        self
    }
    fn entities(&self) -> &dyn EntityContext {
        self
    }
    fn containers(&self) -> &dyn ContainerContext {
        self
    }
    fn recipes(&self) -> &dyn RecipeContext {
        self
    }
}

// ── PluginTestHarness ───────────────────────────────────────────────

/// Test harness for plugin development.
///
/// Provides a simple API for testing plugins without importing
/// internal types. Handles world creation, event bus setup, plugin
/// registration, event dispatch, and command execution.
pub struct PluginTestHarness {
    /// Shared world instance for the test.
    world: Arc<dyn WorldHandle + Send + Sync>,
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
    /// Creates a new test harness with a flat mock world.
    pub fn new() -> Self {
        Self {
            world: Arc::new(MockWorld::flat()),
            instant_bus: EventBus::new(),
            game_bus: EventBus::new(),
            commands: Vec::new(),
            recipes: basalt_recipes::RecipeRegistry::empty(),
        }
    }

    /// Creates a new test harness with the given world.
    pub fn with_world(world: Arc<dyn WorldHandle + Send + Sync>) -> Self {
        Self {
            world,
            instant_bus: EventBus::new(),
            game_bus: EventBus::new(),
            commands: Vec::new(),
            recipes: basalt_recipes::RecipeRegistry::empty(),
        }
    }

    /// Returns a reference to the shared world handle.
    pub fn world(&self) -> &Arc<dyn WorldHandle + Send + Sync> {
        &self.world
    }

    /// Registers a plugin's event handlers and commands.
    ///
    /// Uses a [`NoopContext`] as bootstrap context for any
    /// registry-lifecycle events fired during `on_enable` (e.g.
    /// `RecipeRegisteredEvent`). No real player operations happen
    /// during plugin loading.
    pub fn register(&mut self, plugin: impl Plugin) {
        let mut systems = Vec::new();
        let bootstrap_ctx = NoopContext;
        let mut registrar = PluginRegistrar::new(
            &mut self.instant_bus,
            &mut self.game_bus,
            &mut self.commands,
            &mut systems,
            Arc::clone(&self.world) as Arc<dyn crate::world::handle::WorldHandle + Send + Sync>,
            &mut self.recipes as &mut dyn crate::recipes::RecipeRegistryHandle,
            &bootstrap_ctx as &dyn crate::context::Context,
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
        match E::BUS {
            BusKind::Instant => {
                self.instant_bus.on::<E>(stage, priority, handler);
            }
            BusKind::Game => {
                self.game_bus.on::<E>(stage, priority, handler);
            }
        }
    }

    /// Creates a default harness context for "Steve" with entity ID 1.
    fn context(&self) -> HarnessContext {
        HarnessContext::new(
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

    /// Creates a harness context with custom player identity.
    fn context_for(&self, uuid: Uuid, entity_id: i32, username: &str) -> HarnessContext {
        HarnessContext::new(
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

    /// Dispatches an event with a specific player identity and returns
    /// a [`DispatchResult`].
    pub fn dispatch_as(
        &self,
        event: &mut dyn Event,
        uuid: Uuid,
        entity_id: i32,
        username: &str,
    ) -> DispatchResult {
        let ctx = self.context_for(uuid, entity_id, username);
        self.dispatch_routed(event, &ctx);
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
    fn dispatch_routed(&self, event: &mut dyn Event, ctx: &HarnessContext) {
        let ctx_dyn: &dyn crate::context::Context = ctx;
        match event.bus_kind() {
            BusKind::Instant => self.instant_bus.dispatch_dyn(event, ctx_dyn),
            BusKind::Game => self.game_bus.dispatch_dyn(event, ctx_dyn),
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
    world: Arc<dyn WorldHandle + Send + Sync>,
    /// Unlimited budget for test systems.
    budget: crate::budget::TickBudget,
}

impl SystemTestContext {
    /// Creates a new context with a flat mock world.
    pub fn new() -> Self {
        Self {
            components: std::collections::HashMap::new(),
            entities: std::collections::HashSet::new(),
            next_id: 0,
            world: Arc::new(MockWorld::flat()),
            budget: crate::budget::TickBudget::unlimited(),
        }
    }

    /// Creates a new context with a custom world.
    pub fn with_world(world: Arc<dyn WorldHandle + Send + Sync>) -> Self {
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

impl crate::world::handle::WorldHandle for SystemTestContext {
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
    ) -> Option<crate::world::block_entity::BlockEntity> {
        self.world.get_block_entity(x, y, z)
    }

    fn set_block_entity(
        &self,
        x: i32,
        y: i32,
        z: i32,
        entity: crate::world::block_entity::BlockEntity,
    ) {
        self.world.set_block_entity(x, y, z, entity);
    }

    fn mark_chunk_dirty(&self, cx: i32, cz: i32) {
        self.world.mark_chunk_dirty(cx, cz);
    }

    fn persist_chunk(&self, cx: i32, cz: i32) {
        self.world.persist_chunk(cx, cz);
    }

    fn dirty_chunks(&self) -> Vec<(i32, i32)> {
        self.world.dirty_chunks()
    }

    fn check_overlap(&self, aabb: &crate::world::collision::Aabb) -> bool {
        self.world.check_overlap(aabb)
    }

    fn ray_cast(
        &self,
        origin: (f64, f64, f64),
        direction: (f64, f64, f64),
        max_distance: f64,
    ) -> Option<crate::world::collision::RayHit> {
        self.world.ray_cast(origin, direction, max_distance)
    }

    fn resolve_movement(
        &self,
        aabb: &crate::world::collision::Aabb,
        dx: f64,
        dy: f64,
        dz: f64,
    ) -> (f64, f64, f64) {
        self.world.resolve_movement(aabb, dx, dy, dz)
    }
}

impl crate::system::SystemContext for SystemTestContext {
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
