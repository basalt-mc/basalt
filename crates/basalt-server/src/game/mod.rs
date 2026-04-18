//! Game loop — single dedicated OS thread for all tick-based simulation.
//!
//! Runs at 20 TPS. Each tick:
//! 1. Drains the [`GameInput`] channel (connect, disconnect, movement, blocks, inventory)
//! 2. Runs ECS systems (physics, AI, pathfinding)
//! 3. Produces [`ServerOutput`] game events to player net tasks (zero encoding)

mod blocks;
mod container;
mod dispatch;
mod helpers;
mod inventory;
mod items;
mod lifecycle;
mod movement;
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
        }
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
        for (_, pos) in self.ecs.iter::<basalt_ecs::Position>() {
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
        let mut components = Vec::new();
        {
            let mut registrar = basalt_api::PluginRegistrar::new(
                &mut instant_bus,
                &mut bus,
                &mut commands,
                &mut systems,
                &mut components,
                Arc::clone(&world),
            );
            basalt_plugin_block::BlockPlugin.on_enable(&mut registrar);
            basalt_plugin_drops::DropsPlugin.on_enable(&mut registrar);
        }

        let mut ecs = basalt_ecs::Ecs::new();
        // Register core systems (same as lib.rs)
        ecs.add_system(
            basalt_ecs::SystemBuilder::new("lifetime")
                .phase(basalt_ecs::Phase::Simulate)
                .every(1)
                .run(|ecs: &mut basalt_ecs::Ecs| {
                    for (_, lt) in ecs.iter_mut::<basalt_ecs::Lifetime>() {
                        if lt.remaining_ticks > 0 {
                            lt.remaining_ticks -= 1;
                        }
                    }
                }),
        );
        ecs.add_system(
            basalt_ecs::SystemBuilder::new("pickup_delay")
                .phase(basalt_ecs::Phase::Simulate)
                .every(1)
                .run(|ecs: &mut basalt_ecs::Ecs| {
                    for (_, delay) in ecs.iter_mut::<basalt_ecs::PickupDelay>() {
                        if delay.remaining_ticks > 0 {
                            delay.remaining_ticks -= 1;
                        }
                    }
                }),
        );
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
