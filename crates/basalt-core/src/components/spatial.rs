//! Spatial components: position, rotation, velocity, bounding box.

use super::Component;

/// World position of an entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
    /// Z coordinate.
    pub z: f64,
}
impl Component for Position {}

/// Facing direction of an entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rotation {
    /// Horizontal angle in degrees.
    pub yaw: f32,
    /// Vertical angle in degrees.
    pub pitch: f32,
}
impl Component for Rotation {}

/// Movement vector per tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Velocity {
    /// X movement per tick.
    pub dx: f64,
    /// Y movement per tick.
    pub dy: f64,
    /// Z movement per tick.
    pub dz: f64,
}
impl Component for Velocity {}

/// AABB hitbox dimensions.
///
/// The bounding box is axis-aligned and centered on the entity's
/// position. Used for collision detection and entity interactions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    /// Width (X and Z extent).
    pub width: f32,
    /// Height (Y extent).
    pub height: f32,
}
impl Component for BoundingBox {}

/// Absolute block coordinates (integers).
///
/// Used in events for block interactions (break, place, interact).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockPosition {
    /// Block X coordinate.
    pub x: i32,
    /// Block Y coordinate.
    pub y: i32,
    /// Block Z coordinate.
    pub z: i32,
}

/// Chunk coordinates (X and Z only, no Y).
///
/// Chunks are 16x16 block columns. Chunk coordinates are
/// block coordinates divided by 16 (right-shifted by 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkPosition {
    /// Chunk X coordinate.
    pub x: i32,
    /// Chunk Z coordinate.
    pub z: i32,
}
