//! Long-lived handle to the world runtime.
//!
//! [`WorldHandle`] is the abstract interface plugins use to access world
//! state from anywhere -- captured in system closures, stored in plugin
//! state. The trait itself does not require `Send + Sync` -- concrete
//! implementors that need to be shared across threads (e.g. the runtime
//! `World` wrapped in `Arc`) satisfy these bounds independently.
//! Distinct from [`WorldContext`](crate::context::WorldContext) which
//! extends this trait with dispatch-only methods (response queueing).
//!
//! Implemented by the runtime `World` in `basalt-world` (production)
//! and mock types in tests. Plugins receive an `Arc<dyn WorldHandle>`
//! from [`PluginRegistrar::world`](crate::PluginRegistrar::world).

use crate::world::block_entity::BlockEntity;
use crate::world::collision::{Aabb, RayHit};

/// Long-lived handle to the world runtime.
///
/// Pure read/write operations only -- no response queueing (see
/// [`WorldContext`](crate::context::WorldContext) for that).
///
/// Concrete types stored in `Arc` for cross-thread sharing (e.g.
/// `Arc<World>`) already implement `Send + Sync`. The trait itself
/// does not require them so that per-dispatch types like
/// `ServerContext` (which uses `RefCell`) can implement it too.
pub trait WorldHandle {
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
