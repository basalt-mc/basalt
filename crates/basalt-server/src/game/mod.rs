//! Game loop — single dedicated OS thread for all tick-based simulation.
//!
//! Runs at 20 TPS. Each tick:
//! 1. Drains the [`GameInput`] channel (connect, disconnect, movement, blocks, inventory)
//! 2. Runs ECS systems (physics, AI, pathfinding)
//! 3. Produces [`ServerOutput`] game events to player net tasks (zero encoding)

mod blocks;
mod chunk_stream;
mod click;
mod click_handler;
mod container;
mod crafting;
mod dispatch;
mod helpers;
mod inventory;
mod items;
mod lifecycle;
mod movement;
mod recipe_book;
mod responses;

use std::collections::HashSet;
use std::sync::Arc;

use basalt_api::EventBus;
use tokio::sync::mpsc;

use crate::messages::GameInput;
use crate::net::chunk_cache::ChunkPacketCache;

/// View distance radius in chunks.
const VIEW_RADIUS: i32 = 5;

/// Channel handle for sending output packets to a player's net task.
///
/// Server-internal component (depends on tokio, not in basalt-ecs).
struct OutputHandle {
    /// Sender for the player's output channel.
    tx: mpsc::Sender<crate::messages::ServerOutput>,
}
impl basalt_ecs::Component for OutputHandle {}

/// Whether a player is currently sneaking (shift key held).
///
/// Affects block interaction: sneaking players place blocks instead
/// of opening containers.
struct Sneaking;
impl basalt_ecs::Component for Sneaking {}

/// Mojang skin texture data.
///
/// Server-internal component storing skin properties for broadcasting
/// to other players on join.
struct SkinData {
    /// Mojang profile properties (name, value, signature).
    properties: Vec<basalt_core::broadcast::ProfileProperty>,
}
impl basalt_ecs::Component for SkinData {}

/// Tracks which chunks a player's client currently has loaded.
///
/// Server-internal component for delta-based chunk streaming.
struct ChunkView {
    /// Set of chunk coordinates loaded by the client.
    loaded_chunks: std::collections::HashSet<(i32, i32)>,
}

impl ChunkView {
    /// Creates an empty chunk view.
    fn empty() -> Self {
        Self {
            loaded_chunks: std::collections::HashSet::new(),
        }
    }
}
impl basalt_ecs::Component for ChunkView {}

/// Per-player chunk-batch state: send rate plus pending queue.
///
/// `desired_chunks_per_tick` is seeded at spawn from
/// `ServerSection::chunk_batch_initial_rate` and updated each time
/// the client reports a new decode rate via
/// `ServerboundPlayChunkBatchReceived`. `pending` holds chunks the
/// player needs to receive but hasn't been sent yet — the drainer
/// pops up to `floor(desired_chunks_per_tick)` entries per tick so
/// slow or distant clients aren't flooded.
struct ChunkStreamRate {
    /// Chunks per tick the client can currently decode.
    desired_chunks_per_tick: f32,
    /// Chunks waiting to be sent, drained per tick at the configured rate.
    pending: std::collections::VecDeque<(i32, i32)>,
}

impl ChunkStreamRate {
    /// Creates a rate seeded from the configured initial value with
    /// an empty pending queue.
    fn new(initial_rate: f32) -> Self {
        Self {
            desired_chunks_per_tick: initial_rate,
            pending: std::collections::VecDeque::new(),
        }
    }
}
impl basalt_ecs::Component for ChunkStreamRate {}

/// The game loop state and logic.
pub(crate) struct GameLoop {
    /// Game event bus (blocks, movement, lifecycle).
    pub(super) bus: EventBus,
    /// World — sole owner for writes, concurrent reads by net tasks.
    pub(super) world: Arc<basalt_world::World>,
    /// Shared chunk packet cache — invalidated on block mutations.
    pub(super) chunk_cache: Arc<ChunkPacketCache>,
    /// Entity Component System: entities, components, systems.
    pub(super) ecs: basalt_ecs::Ecs,
    /// Receiver for net task → game loop messages.
    pub(super) game_rx: mpsc::UnboundedReceiver<GameInput>,
    /// Sender for the I/O thread (async chunk persistence).
    pub(super) io_tx: mpsc::UnboundedSender<crate::runtime::io_thread::IoRequest>,
    /// Counter for assigning entity IDs to non-player entities (items, mobs).
    pub(super) next_entity_id: std::sync::Arc<std::sync::atomic::AtomicI32>,
    /// Pre-built DeclareCommands packet payload.
    pub(super) declare_commands: Vec<u8>,
    /// Chunks within simulation distance of any player.
    ///
    /// Updated when players connect, disconnect, or cross chunk boundaries.
    /// ECS systems should only process entities in active chunks.
    pub(super) active_chunks: HashSet<(i32, i32)>,
    /// Simulation distance in chunks around each player.
    pub(super) simulation_distance: i32,
    /// How often to flush dirty chunks to disk, in ticks.
    pub(super) persistence_interval_ticks: u64,
    /// Cycling counter for container window IDs (1-127).
    pub(super) next_window_id: u8,
    /// UUID → EntityId index for O(1) player lookups.
    pub(super) uuid_index: std::collections::HashMap<basalt_types::Uuid, basalt_ecs::EntityId>,
    /// Whether to crash the server when a plugin handler panics.
    pub(super) crash_on_plugin_panic: bool,
    /// Per-player drag state for multi-packet drag operations.
    ///
    /// Tracks in-progress drag operations across multiple WindowClick
    /// packets (StartDrag, AddDragSlot, EndDrag).
    pub(super) drag_states: std::collections::HashMap<basalt_ecs::EntityId, click::DragState>,
    /// Shared recipe registry for crafting grid matching.
    ///
    /// Used by `update_crafting_output()` to match grid contents
    /// against recipes and compute crafting output.
    pub(super) recipes: std::sync::Arc<basalt_recipes::RecipeRegistry>,
    /// Initial chunk-batch rate for newly spawned players, in chunks
    /// per tick. Used to seed each player's [`ChunkStreamRate`].
    pub(super) chunk_batch_initial_rate: f32,
    /// Upper bound applied when the client reports a decode rate.
    /// Reported values are clamped into `[0.01, chunk_batch_max_rate]`.
    pub(super) chunk_batch_max_rate: f32,
}

impl GameLoop {
    /// Creates a new game loop.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bus: EventBus,
        world: Arc<basalt_world::World>,
        chunk_cache: Arc<ChunkPacketCache>,
        game_rx: mpsc::UnboundedReceiver<GameInput>,
        io_tx: mpsc::UnboundedSender<crate::runtime::io_thread::IoRequest>,
        ecs: basalt_ecs::Ecs,
        declare_commands: Vec<u8>,
        next_entity_id: std::sync::Arc<std::sync::atomic::AtomicI32>,
        simulation_distance: i32,
        persistence_interval_ticks: u64,
        crash_on_plugin_panic: bool,
        recipes: std::sync::Arc<basalt_recipes::RecipeRegistry>,
        chunk_batch_initial_rate: f32,
        chunk_batch_max_rate: f32,
    ) -> Self {
        Self {
            bus,
            world,
            chunk_cache,
            ecs,
            game_rx,
            io_tx,
            next_entity_id,
            declare_commands,
            active_chunks: HashSet::new(),
            simulation_distance,
            persistence_interval_ticks,
            next_window_id: 1,
            uuid_index: std::collections::HashMap::new(),
            crash_on_plugin_panic,
            drag_states: std::collections::HashMap::new(),
            recipes,
            chunk_batch_initial_rate,
            chunk_batch_max_rate,
        }
    }

    /// Dispatches an event through the bus, catching plugin panics
    /// if `crash_on_plugin_panic` is false.
    pub(super) fn dispatch_event(
        &self,
        event: &mut dyn basalt_events::Event,
        ctx: &basalt_api::context::ServerContext,
    ) {
        if self.crash_on_plugin_panic {
            self.bus.dispatch_dyn(event, ctx);
        } else {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.bus.dispatch_dyn(event, ctx);
            }));
            if let Err(panic) = result {
                let msg = panic
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| panic.downcast_ref::<String>().map(|s| s.as_str()))
                    .unwrap_or("unknown panic");
                log::error!(target: "basalt::server", "Plugin handler panicked: {msg} — disabling handler");
            }
        }
    }

    /// Associates a UUID with an entity for O(1) lookup.
    pub(super) fn index_uuid(&mut self, uuid: basalt_types::Uuid, entity: basalt_ecs::EntityId) {
        self.uuid_index.insert(uuid, entity);
    }

    /// Finds an entity by UUID. O(1).
    pub(super) fn find_by_uuid(&self, uuid: basalt_types::Uuid) -> Option<basalt_ecs::EntityId> {
        self.uuid_index.get(&uuid).copied()
    }

    /// Removes a UUID from the index.
    pub(super) fn remove_uuid(&mut self, uuid: basalt_types::Uuid) {
        self.uuid_index.remove(&uuid);
    }

    /// Allocates the next container window ID (1-127, cycling).
    pub(super) fn alloc_window_id(&mut self) -> u8 {
        let id = self.next_window_id;
        self.next_window_id = if id >= 127 { 1 } else { id + 1 };
        id
    }

    /// Processes one tick.
    pub fn tick(&mut self, tick: u64) {
        self.drain_game_input();
        self.ecs.run_all(tick);
        self.broadcast_item_movement();
        self.tick_item_pickup();
        self.collect_expired_entities();
        // Drain queued chunks AFTER input + ECS systems so newly-enqueued
        // chunks (from boundary crossings handled this tick) are eligible
        // for sending immediately.
        self.drain_chunk_batches();
        self.flush_dirty_chunks_if_due(tick);
    }

    /// Periodically flushes all dirty chunks to the I/O thread.
    ///
    /// Runs every `persistence_interval_ticks` ticks (~30s at 20 TPS).
    /// Collects dirty chunks from the World and sends them as batch
    /// persist requests to the I/O thread.
    fn flush_dirty_chunks_if_due(&self, tick: u64) {
        if self.persistence_interval_ticks == 0
            || !tick.is_multiple_of(self.persistence_interval_ticks)
        {
            return;
        }
        let dirty = self.world.dirty_chunks();
        if dirty.is_empty() {
            return;
        }
        log::debug!(target: "basalt::game", "Flushing {} dirty chunks to disk", dirty.len());
        for (cx, cz) in dirty {
            let _ = self
                .io_tx
                .send(crate::runtime::io_thread::IoRequest::PersistChunk { cx, cz });
        }
    }

    /// Recalculates the set of active chunks from all player positions.
    ///
    /// A chunk is active if it falls within `simulation_distance` of
    /// any connected player.
    pub(super) fn rebuild_active_chunks(&mut self) {
        self.active_chunks.clear();
        let sd = self.simulation_distance;
        for (_, pos) in self.ecs.iter::<basalt_core::Position>() {
            // Only count entities that are players
            let cx = (pos.x as i32) >> 4;
            let cz = (pos.z as i32) >> 4;
            for dx in -sd..=sd {
                for dz in -sd..=sd {
                    self.active_chunks.insert((cx + dx, cz + dz));
                }
            }
        }
    }

    /// Returns whether the chunk at (cx, cz) is within simulation distance.
    #[allow(dead_code)]
    pub fn is_chunk_active(&self, cx: i32, cz: i32) -> bool {
        self.active_chunks.contains(&(cx, cz))
    }
}

#[cfg(test)]
pub(super) mod tests {
    use std::sync::Arc;

    use basalt_api::{EventBus, Plugin};
    use basalt_types::Uuid;
    use tokio::sync::mpsc;

    use super::GameLoop;
    use crate::messages::{GameInput, ServerOutput};
    use crate::net::chunk_cache::ChunkPacketCache;

    pub(super) fn test_game_loop() -> (
        GameLoop,
        mpsc::UnboundedSender<GameInput>,
        mpsc::UnboundedReceiver<crate::runtime::io_thread::IoRequest>,
    ) {
        let world = Arc::new(basalt_world::World::new_memory(42));
        let chunk_cache = Arc::new(ChunkPacketCache::new(Arc::clone(&world), 256));
        let (game_tx, game_rx) = mpsc::unbounded_channel();
        let (io_tx, io_rx) = mpsc::unbounded_channel();

        let mut instant_bus = EventBus::new();
        let mut bus = EventBus::new();
        let mut commands = Vec::new();
        let mut systems = Vec::new();
        let mut recipes = basalt_recipes::RecipeRegistry::with_vanilla();
        let bootstrap_ctx = basalt_api::context::ServerContext::new(
            Arc::clone(&world),
            basalt_core::player::PlayerInfo::stub(),
        );
        {
            let mut registrar = basalt_api::PluginRegistrar::new(
                &mut instant_bus,
                &mut bus,
                &mut commands,
                &mut systems,
                Arc::clone(&world),
                &mut recipes,
                &bootstrap_ctx,
            );
            basalt_plugin_block::BlockPlugin.on_enable(&mut registrar);
            basalt_plugin_item::ItemPlugin.on_enable(&mut registrar);
            basalt_plugin_container::ContainerPlugin.on_enable(&mut registrar);
            basalt_plugin_recipe::RecipePlugin.on_enable(&mut registrar);
        }

        let mut ecs = basalt_ecs::Ecs::new();
        ecs.set_world(Arc::clone(&world));
        // Register core systems (same as lib.rs)
        ecs.add_system(
            basalt_ecs::SystemBuilder::new("lifetime")
                .phase(basalt_ecs::Phase::Simulate)
                .every(1)
                .reads::<basalt_core::Lifetime>()
                .writes::<basalt_core::Lifetime>()
                .run(|ctx: &mut dyn basalt_core::SystemContext| {
                    use basalt_core::SystemContextExt;
                    for id in ctx.query::<basalt_core::Lifetime>() {
                        if let Some(lt) = ctx.get_mut::<basalt_core::Lifetime>(id)
                            && lt.remaining_ticks > 0
                        {
                            lt.remaining_ticks -= 1;
                        }
                    }
                }),
        );
        ecs.add_system(
            basalt_ecs::SystemBuilder::new("pickup_delay")
                .phase(basalt_ecs::Phase::Simulate)
                .every(1)
                .reads::<basalt_core::PickupDelay>()
                .writes::<basalt_core::PickupDelay>()
                .run(|ctx: &mut dyn basalt_core::SystemContext| {
                    use basalt_core::SystemContextExt;
                    for id in ctx.query::<basalt_core::PickupDelay>() {
                        if let Some(delay) = ctx.get_mut::<basalt_core::PickupDelay>(id)
                            && delay.remaining_ticks > 0
                        {
                            delay.remaining_ticks -= 1;
                        }
                    }
                }),
        );
        let recipes_arc = Arc::new(recipes);
        let game_loop = GameLoop::new(
            bus,
            world,
            chunk_cache,
            game_rx,
            io_tx,
            ecs,
            Vec::new(),
            Arc::new(std::sync::atomic::AtomicI32::new(1000)),
            8,
            0,
            true,
            recipes_arc,
            25.0,
            100.0,
        );
        (game_loop, game_tx, io_rx)
    }

    pub(super) fn connect_player(
        game_loop: &mut GameLoop,
        game_tx: &mpsc::UnboundedSender<GameInput>,
        uuid: Uuid,
        entity_id: i32,
    ) -> mpsc::Receiver<ServerOutput> {
        let (output_tx, output_rx) = mpsc::channel(256);
        let _ = game_tx.send(GameInput::PlayerConnected {
            entity_id,
            uuid,
            username: "Steve".into(),
            skin_properties: vec![],
            position: (0.0, -60.0, 0.0),
            yaw: 0.0,
            pitch: 0.0,
            output_tx,
        });
        game_loop.tick(0);
        output_rx
    }
}
