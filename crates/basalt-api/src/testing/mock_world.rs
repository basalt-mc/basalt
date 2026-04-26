//! In-memory mock world for testing.
//!
//! Provides a [`MockWorld`] that implements [`WorldHandle`] with simple
//! `HashMap` storage. Used by [`PluginTestHarness`](super::PluginTestHarness)
//! and [`SystemTestContext`](super::SystemTestContext) so that basalt-api
//! tests do not depend on the concrete `basalt_world::World`.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::world::block;
use crate::world::block_entity::BlockEntity;
use crate::world::collision::{self, Aabb, RayHit};
use crate::world::handle::WorldHandle;

/// In-memory mock world for test harnesses.
///
/// Stores blocks and block entities in hash maps protected by mutexes.
/// Generates flat terrain on first access (bedrock at y=-64, dirt at
/// y=-63..-62, grass at y=-61) to match the standard superflat layout
/// used by most plugin tests.
pub struct MockWorld {
    /// Block states keyed by (x, y, z).
    blocks: Mutex<HashMap<(i32, i32, i32), u16>>,
    /// Block entities keyed by (x, y, z).
    block_entities: Mutex<HashMap<(i32, i32, i32), BlockEntity>>,
    /// Chunks marked as dirty, keyed by (cx, cz).
    dirty: Mutex<std::collections::HashSet<(i32, i32)>>,
    /// Whether to generate flat terrain on access.
    flat: bool,
}

impl MockWorld {
    /// Creates a new mock world with flat terrain generation.
    pub fn flat() -> Self {
        Self {
            blocks: Mutex::new(HashMap::new()),
            block_entities: Mutex::new(HashMap::new()),
            dirty: Mutex::new(std::collections::HashSet::new()),
            flat: true,
        }
    }

    /// Creates a new empty mock world (all air).
    pub fn empty() -> Self {
        Self {
            blocks: Mutex::new(HashMap::new()),
            block_entities: Mutex::new(HashMap::new()),
            dirty: Mutex::new(std::collections::HashSet::new()),
            flat: false,
        }
    }

    /// Returns the block state, generating flat terrain if configured.
    fn block_at(&self, x: i32, y: i32, z: i32) -> u16 {
        let map = self.blocks.lock().unwrap();
        if let Some(&state) = map.get(&(x, y, z)) {
            return state;
        }
        if self.flat {
            return match y {
                -64 => block::BEDROCK,
                -63 | -62 => block::DIRT,
                -61 => block::GRASS_BLOCK,
                _ => block::AIR,
            };
        }
        block::AIR
    }
}

impl WorldHandle for MockWorld {
    fn get_block(&self, x: i32, y: i32, z: i32) -> u16 {
        self.block_at(x, y, z)
    }

    fn set_block(&self, x: i32, y: i32, z: i32, state: u16) {
        self.blocks.lock().unwrap().insert((x, y, z), state);
        let cx = x >> 4;
        let cz = z >> 4;
        self.dirty.lock().unwrap().insert((cx, cz));
    }

    fn get_block_entity(&self, x: i32, y: i32, z: i32) -> Option<BlockEntity> {
        self.block_entities.lock().unwrap().get(&(x, y, z)).cloned()
    }

    fn set_block_entity(&self, x: i32, y: i32, z: i32, entity: BlockEntity) {
        self.block_entities
            .lock()
            .unwrap()
            .insert((x, y, z), entity);
    }

    fn mark_chunk_dirty(&self, cx: i32, cz: i32) {
        self.dirty.lock().unwrap().insert((cx, cz));
    }

    fn persist_chunk(&self, _cx: i32, _cz: i32) {
        // No-op for mock — no disk persistence.
    }

    fn dirty_chunks(&self) -> Vec<(i32, i32)> {
        self.dirty.lock().unwrap().iter().copied().collect()
    }

    fn check_overlap(&self, aabb: &Aabb) -> bool {
        collision::check_overlap(self, aabb)
    }

    fn ray_cast(
        &self,
        origin: (f64, f64, f64),
        direction: (f64, f64, f64),
        max_distance: f64,
    ) -> Option<RayHit> {
        collision::ray_cast(self, origin, direction, max_distance)
    }

    fn resolve_movement(&self, aabb: &Aabb, dx: f64, dy: f64, dz: f64) -> (f64, f64, f64) {
        collision::resolve_movement(self, aabb, dx, dy, dz)
    }
}
