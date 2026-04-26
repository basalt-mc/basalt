//! WorldHandle implementation for the World runtime.
//!
//! Bridges the abstract [`WorldHandle`] trait (defined in basalt-api)
//! with the concrete [`World`] struct. This is the production
//! implementation used by the server; test code uses mock impls.

use basalt_api::world::block_entity::BlockEntity;
use basalt_api::world::collision::{Aabb, RayHit};
use basalt_api::world::handle::WorldHandle;

use crate::World;

impl WorldHandle for World {
    fn get_block(&self, x: i32, y: i32, z: i32) -> u16 {
        World::get_block(self, x, y, z)
    }

    fn set_block(&self, x: i32, y: i32, z: i32, state: u16) {
        World::set_block(self, x, y, z, state);
    }

    fn get_block_entity(&self, x: i32, y: i32, z: i32) -> Option<BlockEntity> {
        World::get_block_entity(self, x, y, z).map(|r| r.value().clone())
    }

    fn set_block_entity(&self, x: i32, y: i32, z: i32, entity: BlockEntity) {
        World::set_block_entity(self, x, y, z, entity);
    }

    fn mark_chunk_dirty(&self, cx: i32, cz: i32) {
        World::mark_chunk_dirty(self, cx, cz);
    }

    fn persist_chunk(&self, cx: i32, cz: i32) {
        World::persist_chunk(self, cx, cz);
    }

    fn dirty_chunks(&self) -> Vec<(i32, i32)> {
        World::dirty_chunks(self)
    }

    fn check_overlap(&self, aabb: &Aabb) -> bool {
        basalt_api::world::collision::check_overlap(self, aabb)
    }

    fn ray_cast(
        &self,
        origin: (f64, f64, f64),
        direction: (f64, f64, f64),
        max_distance: f64,
    ) -> Option<RayHit> {
        basalt_api::world::collision::ray_cast(self, origin, direction, max_distance)
    }

    fn resolve_movement(&self, aabb: &Aabb, dx: f64, dy: f64, dz: f64) -> (f64, f64, f64) {
        basalt_api::world::collision::resolve_movement(self, aabb, dx, dy, dz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use basalt_api::world::block;

    /// Verifies that `World` satisfies `WorldHandle` and the trait
    /// methods delegate correctly to the concrete implementation.
    #[test]
    fn world_implements_world_handle() {
        let world = World::flat();
        let handle: &dyn WorldHandle = &world;

        // Flat world: bedrock at y=-64
        assert_eq!(handle.get_block(0, -64, 0), block::BEDROCK);
        assert_eq!(handle.get_block(0, -60, 0), block::AIR);

        // set_block round-trip
        handle.set_block(5, 64, 3, block::STONE);
        assert_eq!(handle.get_block(5, 64, 3), block::STONE);
    }

    /// Verifies that collision methods work through the trait.
    #[test]
    fn world_handle_collision_methods() {
        let world = World::flat();
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
        let world = World::flat();
        let handle: &dyn WorldHandle = &world;

        let hit = handle.ray_cast((0.5, -50.0, 0.5), (0.0, -1.0, 0.0), 20.0);
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().block_y, -61);
    }

    /// Verifies that dirty chunk tracking works through the trait.
    #[test]
    fn world_handle_dirty_chunks() {
        let world = World::flat();
        let handle: &dyn WorldHandle = &world;

        handle.set_block(0, 64, 0, block::STONE);
        let dirty = handle.dirty_chunks();
        assert!(dirty.contains(&(0, 0)));
    }

    /// Verifies block entity operations through the trait.
    #[test]
    fn world_handle_block_entities() {
        let world = World::flat();
        let handle: &dyn WorldHandle = &world;

        assert!(handle.get_block_entity(0, 64, 0).is_none());

        handle.set_block_entity(0, 64, 0, BlockEntity::empty_chest());
        let be = handle.get_block_entity(0, 64, 0);
        assert!(be.is_some());
        assert!(matches!(be.unwrap(), BlockEntity::Chest { .. }));
    }
}
