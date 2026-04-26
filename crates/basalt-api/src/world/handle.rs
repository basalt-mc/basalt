//! Long-lived handle to the world runtime.
//!
//! [`WorldHandle`] is the abstract interface plugins use to access world
//! state from anywhere -- captured in system closures, stored in plugin
//! state, shared across threads. Distinct from
//! [`WorldContext`](crate::context::WorldContext) which extends this trait
//! with dispatch-only methods (response queueing).
//!
//! Implemented by [`basalt_world::World`] (production) and mock types in
//! tests. Plugins receive an `Arc<dyn WorldHandle>` from
//! [`PluginRegistrar::world`](crate::PluginRegistrar::world).

use crate::world::block_entity::BlockEntity;
use crate::world::collision::{Aabb, RayHit};

/// Long-lived handle to the world runtime.
///
/// `Send + Sync + 'static` so it can be captured in system closures
/// and passed across threads. Pure read/write operations only -- no
/// response queueing (see [`WorldContext`](crate::context::WorldContext)
/// for that).
pub trait WorldHandle: Send + Sync {
    /// Returns the block state at the given position.
    ///
    /// Generates or loads the chunk if it is not cached. Returns `0`
    /// (air) for positions outside the valid Y range.
    fn get_block(&self, x: i32, y: i32, z: i32) -> u16;

    /// Sets a block state at the given position.
    ///
    /// Generates or loads the chunk if it is not cached. Marks the
    /// containing chunk as dirty for persistence.
    fn set_block(&self, x: i32, y: i32, z: i32, state: u16);

    /// Returns a cloned block entity at the given position, if any.
    fn get_block_entity(&self, x: i32, y: i32, z: i32) -> Option<BlockEntity>;

    /// Sets a block entity at the given position. Marks the chunk dirty.
    fn set_block_entity(&self, x: i32, y: i32, z: i32, entity: BlockEntity);

    /// Marks a chunk as dirty so the persistence system flushes it
    /// on the next batch.
    fn mark_chunk_dirty(&self, cx: i32, cz: i32);

    /// Forces immediate persistence of the chunk to disk (synchronous).
    ///
    /// Most callers should prefer [`mark_chunk_dirty`](Self::mark_chunk_dirty)
    /// to let the batch persistence path handle it.
    fn persist_chunk(&self, cx: i32, cz: i32);

    /// Returns the coordinates of all chunks currently marked dirty.
    fn dirty_chunks(&self) -> Vec<(i32, i32)>;

    /// Returns `true` if the AABB overlaps any solid block.
    fn check_overlap(&self, aabb: &Aabb) -> bool;

    /// Casts a ray through solid blocks. Returns the first hit within
    /// `max_distance`, or `None`.
    fn ray_cast(
        &self,
        origin: (f64, f64, f64),
        direction: (f64, f64, f64),
        max_distance: f64,
    ) -> Option<RayHit>;

    /// Resolves desired AABB movement against solid block collisions.
    /// Returns the actual `(dx, dy, dz)` after clamping.
    fn resolve_movement(&self, aabb: &Aabb, dx: f64, dy: f64, dz: f64) -> (f64, f64, f64);
}

// ── Impl for basalt_world::World ─────────────────────────────────────
//
// This impl lives in basalt-api temporarily. It moves to basalt-world
// in a later task (when basalt-world gains a basalt-api dep). Permitted
// by Rust's orphan rule because `WorldHandle` is local to basalt-api.

impl WorldHandle for basalt_world::World {
    fn get_block(&self, x: i32, y: i32, z: i32) -> u16 {
        basalt_world::World::get_block(self, x, y, z)
    }

    fn set_block(&self, x: i32, y: i32, z: i32, state: u16) {
        basalt_world::World::set_block(self, x, y, z, state);
    }

    fn get_block_entity(&self, x: i32, y: i32, z: i32) -> Option<BlockEntity> {
        basalt_world::World::get_block_entity(self, x, y, z).map(|r| r.value().clone())
    }

    fn set_block_entity(&self, x: i32, y: i32, z: i32, entity: BlockEntity) {
        basalt_world::World::set_block_entity(self, x, y, z, entity);
    }

    fn mark_chunk_dirty(&self, cx: i32, cz: i32) {
        basalt_world::World::mark_chunk_dirty(self, cx, cz);
    }

    fn persist_chunk(&self, cx: i32, cz: i32) {
        basalt_world::World::persist_chunk(self, cx, cz);
    }

    fn dirty_chunks(&self) -> Vec<(i32, i32)> {
        basalt_world::World::dirty_chunks(self)
    }

    fn check_overlap(&self, aabb: &Aabb) -> bool {
        crate::world::collision::check_overlap(self, aabb)
    }

    fn ray_cast(
        &self,
        origin: (f64, f64, f64),
        direction: (f64, f64, f64),
        max_distance: f64,
    ) -> Option<RayHit> {
        crate::world::collision::ray_cast(self, origin, direction, max_distance)
    }

    fn resolve_movement(&self, aabb: &Aabb, dx: f64, dy: f64, dz: f64) -> (f64, f64, f64) {
        crate::world::collision::resolve_movement(self, aabb, dx, dy, dz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that `World` satisfies `WorldHandle` and the trait
    /// methods delegate correctly to the concrete implementation.
    #[test]
    fn world_implements_world_handle() {
        let world = basalt_world::World::flat();
        let handle: &dyn WorldHandle = &world;

        // Flat world: bedrock at y=-64
        assert_eq!(handle.get_block(0, -64, 0), basalt_world::block::BEDROCK);
        assert_eq!(handle.get_block(0, -60, 0), basalt_world::block::AIR);

        // set_block round-trip
        handle.set_block(5, 64, 3, basalt_world::block::STONE);
        assert_eq!(handle.get_block(5, 64, 3), basalt_world::block::STONE);
    }

    /// Verifies that collision methods work through the trait.
    #[test]
    fn world_handle_collision_methods() {
        let world = basalt_world::World::flat();
        let handle: &dyn WorldHandle = &world;

        // AABB overlapping solid ground
        let aabb = Aabb::from_entity(0.0, -62.0, 0.0, 0.6, 1.8);
        assert!(handle.check_overlap(&aabb));

        // AABB in open air
        let air_aabb = Aabb::from_entity(0.0, -50.0, 0.0, 0.6, 1.8);
        assert!(!handle.check_overlap(&air_aabb));

        // resolve_movement stops at ground
        let standing = Aabb::from_entity(0.0, -60.0, 0.0, 0.6, 1.8);
        let (_, dy, _) = handle.resolve_movement(&standing, 0.0, -1.0, 0.0);
        assert_eq!(dy, 0.0);
    }

    /// Verifies ray casting through the trait.
    #[test]
    fn world_handle_ray_cast() {
        let world = basalt_world::World::flat();
        let handle: &dyn WorldHandle = &world;

        let hit = handle.ray_cast((0.5, -50.0, 0.5), (0.0, -1.0, 0.0), 20.0);
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().block_y, -61);
    }

    /// Verifies that dirty chunk tracking works through the trait.
    #[test]
    fn world_handle_dirty_chunks() {
        let world = basalt_world::World::flat();
        let handle: &dyn WorldHandle = &world;

        handle.set_block(0, 64, 0, basalt_world::block::STONE);
        let dirty = handle.dirty_chunks();
        assert!(dirty.contains(&(0, 0)));
    }

    /// Verifies block entity operations through the trait.
    #[test]
    fn world_handle_block_entities() {
        let world = basalt_world::World::flat();
        let handle: &dyn WorldHandle = &world;

        assert!(handle.get_block_entity(0, 64, 0).is_none());

        handle.set_block_entity(0, 64, 0, BlockEntity::empty_chest());
        let be = handle.get_block_entity(0, 64, 0);
        assert!(be.is_some());
        assert!(matches!(be.unwrap(), BlockEntity::Chest { .. }));
    }
}
