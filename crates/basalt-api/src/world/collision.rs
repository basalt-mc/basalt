//! Collision utilities for physics simulation.
//!
//! Provides AABB-vs-block collision detection and ray casting against
//! the block grid. These are the building blocks for the physics system
//! and future gameplay mechanics (line-of-sight, block targeting).

use crate::world::block::is_solid;
use crate::world::handle::WorldHandle;

/// An axis-aligned bounding box in world coordinates.
#[derive(Debug, Clone, Copy)]
pub struct Aabb {
    /// Minimum corner (lowest X, Y, Z).
    pub min_x: f64,
    /// Minimum Y.
    pub min_y: f64,
    /// Minimum Z.
    pub min_z: f64,
    /// Maximum corner (highest X, Y, Z).
    pub max_x: f64,
    /// Maximum Y.
    pub max_y: f64,
    /// Maximum Z.
    pub max_z: f64,
}

impl Aabb {
    /// Creates an AABB centered at `(x, y, z)` with the given dimensions.
    ///
    /// The box extends `width/2` in X and Z from center, and `height`
    /// upward from `y` (entity position is at feet level).
    pub fn from_entity(x: f64, y: f64, z: f64, width: f32, height: f32) -> Self {
        let hw = f64::from(width) / 2.0;
        let h = f64::from(height);
        Self {
            min_x: x - hw,
            min_y: y,
            min_z: z - hw,
            max_x: x + hw,
            max_y: y + h,
            max_z: z + hw,
        }
    }

    /// Returns this AABB offset by the given delta.
    pub fn offset(&self, dx: f64, dy: f64, dz: f64) -> Self {
        Self {
            min_x: self.min_x + dx,
            min_y: self.min_y + dy,
            min_z: self.min_z + dz,
            max_x: self.max_x + dx,
            max_y: self.max_y + dy,
            max_z: self.max_z + dz,
        }
    }

    /// Returns whether this AABB overlaps a unit block at (bx, by, bz).
    fn overlaps_block(&self, bx: i32, by: i32, bz: i32) -> bool {
        let bx = bx as f64;
        let by = by as f64;
        let bz = bz as f64;
        self.max_x > bx
            && self.min_x < bx + 1.0
            && self.max_y > by
            && self.min_y < by + 1.0
            && self.max_z > bz
            && self.min_z < bz + 1.0
    }
}

/// Checks if an AABB overlaps any solid block in the world.
///
/// Iterates all block positions that the AABB spans and returns
/// `true` if any of them are solid. Used for ground detection and
/// simple collision checks.
pub fn check_overlap(world: &dyn WorldHandle, aabb: &Aabb) -> bool {
    let min_bx = aabb.min_x.floor() as i32;
    let min_by = aabb.min_y.floor() as i32;
    let min_bz = aabb.min_z.floor() as i32;
    let max_bx = aabb.max_x.ceil() as i32;
    let max_by = aabb.max_y.ceil() as i32;
    let max_bz = aabb.max_z.ceil() as i32;

    for bx in min_bx..max_bx {
        for by in min_by..max_by {
            for bz in min_bz..max_bz {
                if is_solid(world.get_block(bx, by, bz)) && aabb.overlaps_block(bx, by, bz) {
                    return true;
                }
            }
        }
    }
    false
}

/// Result of a ray cast against the block grid.
#[derive(Debug, Clone)]
pub struct RayHit {
    /// The block position that was hit.
    pub block_x: i32,
    /// Block Y.
    pub block_y: i32,
    /// Block Z.
    pub block_z: i32,
    /// Distance from origin to hit point.
    pub distance: f64,
}

/// Casts a ray through the world and returns the first solid block hit.
///
/// Uses a simple stepping algorithm along the ray direction.
/// Returns `None` if no solid block is found within `max_distance`.
pub fn ray_cast(
    world: &dyn WorldHandle,
    origin: (f64, f64, f64),
    direction: (f64, f64, f64),
    max_distance: f64,
) -> Option<RayHit> {
    let (origin_x, origin_y, origin_z) = origin;
    let (dir_x, dir_y, dir_z) = direction;
    let step = 0.1;
    let steps = (max_distance / step) as usize;
    let len = (dir_x * dir_x + dir_y * dir_y + dir_z * dir_z).sqrt();
    if len < 1e-10 {
        return None;
    }
    let (nx, ny, nz) = (dir_x / len, dir_y / len, dir_z / len);

    for i in 0..=steps {
        let d = i as f64 * step;
        let x = origin_x + nx * d;
        let y = origin_y + ny * d;
        let z = origin_z + nz * d;
        let bx = x.floor() as i32;
        let by = y.floor() as i32;
        let bz = z.floor() as i32;

        if is_solid(world.get_block(bx, by, bz)) {
            return Some(RayHit {
                block_x: bx,
                block_y: by,
                block_z: bz,
                distance: d,
            });
        }
    }
    None
}

/// Resolves movement of an AABB against solid blocks.
///
/// Takes the entity's AABB and desired velocity, returns the actual
/// velocity after clamping against solid blocks. Each axis is resolved
/// independently (Y first for gravity, then X, then Z).
pub fn resolve_movement(
    world: &dyn WorldHandle,
    aabb: &Aabb,
    dx: f64,
    dy: f64,
    dz: f64,
) -> (f64, f64, f64) {
    let mut resolved_dy = dy;
    let mut resolved_dx = dx;
    let mut resolved_dz = dz;

    // Resolve Y axis first (gravity is most important)
    if resolved_dy != 0.0 {
        let test = aabb.offset(0.0, resolved_dy, 0.0);
        if check_overlap(world, &test) {
            // Clamp to the nearest block boundary
            if resolved_dy < 0.0 {
                // Falling: snap to top of block below
                resolved_dy = (aabb.min_y.floor() - aabb.min_y).max(resolved_dy);
                // If still overlapping, zero out
                if check_overlap(world, &aabb.offset(0.0, resolved_dy, 0.0)) {
                    resolved_dy = 0.0;
                }
            } else {
                // Rising: snap to bottom of block above
                resolved_dy = (aabb.max_y.ceil() - aabb.max_y).min(resolved_dy);
                if check_overlap(world, &aabb.offset(0.0, resolved_dy, 0.0)) {
                    resolved_dy = 0.0;
                }
            }
        }
    }

    let aabb_after_y = aabb.offset(0.0, resolved_dy, 0.0);

    // Resolve X axis
    if resolved_dx != 0.0 {
        let test = aabb_after_y.offset(resolved_dx, 0.0, 0.0);
        if check_overlap(world, &test) {
            resolved_dx = 0.0;
        }
    }

    // Resolve Z axis
    if resolved_dz != 0.0 {
        let test = aabb_after_y.offset(resolved_dx, 0.0, resolved_dz);
        if check_overlap(world, &test) {
            resolved_dz = 0.0;
        }
    }

    (resolved_dx, resolved_dy, resolved_dz)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::block;
    use crate::world::block_entity::BlockEntity;

    /// Minimal flat-world mock for collision tests.
    ///
    /// Returns solid blocks (bedrock/dirt/grass) at y <= -61 and air
    /// above, matching the standard superflat layout.
    struct FlatMock;

    impl WorldHandle for FlatMock {
        fn get_block(&self, _x: i32, y: i32, _z: i32) -> u16 {
            match y {
                -64 => block::BEDROCK,
                -63 | -62 => block::DIRT,
                -61 => block::GRASS_BLOCK,
                _ => block::AIR,
            }
        }
        fn set_block(&self, _x: i32, _y: i32, _z: i32, _state: u16) {}
        fn get_block_entity(&self, _x: i32, _y: i32, _z: i32) -> Option<BlockEntity> {
            None
        }
        fn set_block_entity(&self, _x: i32, _y: i32, _z: i32, _entity: BlockEntity) {}
        fn mark_chunk_dirty(&self, _cx: i32, _cz: i32) {}
        fn persist_chunk(&self, _cx: i32, _cz: i32) {}
        fn dirty_chunks(&self) -> Vec<(i32, i32)> {
            Vec::new()
        }
        fn check_overlap(&self, aabb: &Aabb) -> bool {
            check_overlap(self, aabb)
        }
        fn ray_cast(
            &self,
            origin: (f64, f64, f64),
            direction: (f64, f64, f64),
            max_distance: f64,
        ) -> Option<RayHit> {
            ray_cast(self, origin, direction, max_distance)
        }
        fn resolve_movement(&self, aabb: &Aabb, dx: f64, dy: f64, dz: f64) -> (f64, f64, f64) {
            resolve_movement(self, aabb, dx, dy, dz)
        }
    }

    fn test_world() -> FlatMock {
        FlatMock
    }

    #[test]
    fn aabb_from_entity() {
        let aabb = Aabb::from_entity(0.0, -60.0, 0.0, 0.6, 1.8);
        // f32->f64 conversion causes small precision loss, use relaxed tolerance
        assert!(aabb.min_x < 0.0, "min_x should be negative");
        assert!((aabb.min_y - (-60.0)).abs() < 1e-6);
        assert!((aabb.max_y - (-58.2)).abs() < 1e-4);
    }

    #[test]
    fn check_overlap_detects_solid() {
        let world = test_world();
        // AABB at y=-62 overlaps dirt block at y=-62
        let aabb = Aabb::from_entity(0.0, -62.0, 0.0, 0.6, 1.8);
        assert!(check_overlap(&world, &aabb));
    }

    #[test]
    fn check_overlap_no_collision_in_air() {
        let world = test_world();
        // AABB at y=-60 (above grass at -61) -- all air
        let aabb = Aabb::from_entity(0.0, -60.0, 0.0, 0.6, 1.8);
        assert!(!check_overlap(&world, &aabb));
    }

    #[test]
    fn ray_cast_finds_ground() {
        let world = test_world();
        // Cast straight down from y=-50
        let hit = ray_cast(&world, (0.5, -50.0, 0.5), (0.0, -1.0, 0.0), 20.0);
        assert!(hit.is_some());
        let hit = hit.unwrap();
        assert_eq!(hit.block_y, -61); // Grass layer
    }

    #[test]
    fn ray_cast_misses_in_air() {
        let world = test_world();
        // Cast horizontally at y=-50 -- all air
        let hit = ray_cast(&world, (0.5, -50.0, 0.5), (1.0, 0.0, 0.0), 5.0);
        assert!(hit.is_none());
    }

    #[test]
    fn resolve_movement_stops_at_ground() {
        let world = test_world();
        // Entity at y=-60 (just above grass), falling
        let aabb = Aabb::from_entity(0.0, -60.0, 0.0, 0.6, 1.8);
        let (dx, dy, dz) = resolve_movement(&world, &aabb, 0.0, -1.0, 0.0);
        assert_eq!(dx, 0.0);
        assert_eq!(dy, 0.0); // Stopped by ground
        assert_eq!(dz, 0.0);
    }

    #[test]
    fn resolve_movement_allows_free_fall() {
        let world = test_world();
        // Entity at y=-50 (high in air), falling
        let aabb = Aabb::from_entity(0.0, -50.0, 0.0, 0.6, 1.8);
        let (_, dy, _) = resolve_movement(&world, &aabb, 0.0, -0.5, 0.0);
        assert!((dy - (-0.5)).abs() < f64::EPSILON);
    }

    #[test]
    fn resolve_movement_stops_horizontal() {
        let world = test_world();
        // Entity overlapping ground at y=-62, horizontal blocked
        let aabb = Aabb::from_entity(0.0, -62.0, 0.0, 0.6, 1.8);
        let (dx, _, _) = resolve_movement(&world, &aabb, 1.0, 0.0, 0.0);
        assert_eq!(dx, 0.0);
    }
}
