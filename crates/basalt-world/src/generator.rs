//! World generators for creating terrain.
//!
//! Generators populate `ChunkColumn` instances with blocks based on
//! their coordinates and a seed. The `FlatWorldGenerator` creates a
//! simple superflat world; future generators can add noise-based
//! terrain, biomes, and structures.

use crate::block;
use crate::chunk::ChunkColumn;

/// Generates a superflat world with fixed layers.
///
/// The terrain consists of:
/// - y = -64: bedrock
/// - y = -63 to -62: dirt
/// - y = -61: grass_block
/// - y = -60 and above: air
///
/// This gives a ground level at y = -61, which is the standard
/// superflat world height. The spawn point should be set to y = -60.
pub struct FlatWorldGenerator;

impl FlatWorldGenerator {
    /// The Y coordinate of the ground surface (top of grass).
    pub const GROUND_Y: i32 = -61;

    /// The Y coordinate where players should spawn (one above ground).
    pub const SPAWN_Y: i32 = -60;

    /// Populates a chunk column with flat terrain layers.
    pub fn generate(&self, chunk: &mut ChunkColumn) {
        for x in 0..16 {
            for z in 0..16 {
                // Bedrock at the very bottom
                chunk.set_block(x, -64, z, block::BEDROCK);
                // Dirt layers
                chunk.set_block(x, -63, z, block::DIRT);
                chunk.set_block(x, -62, z, block::DIRT);
                // Grass on top
                chunk.set_block(x, -61, z, block::GRASS_BLOCK);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_world_has_bedrock_bottom() {
        let generator = FlatWorldGenerator;
        let mut chunk = ChunkColumn::new(0, 0);
        generator.generate(&mut chunk);
        assert_eq!(chunk.get_block(0, -64, 0), block::BEDROCK);
    }

    #[test]
    fn flat_world_has_dirt_layers() {
        let generator = FlatWorldGenerator;
        let mut chunk = ChunkColumn::new(0, 0);
        generator.generate(&mut chunk);
        assert_eq!(chunk.get_block(0, -63, 0), block::DIRT);
        assert_eq!(chunk.get_block(0, -62, 0), block::DIRT);
    }

    #[test]
    fn flat_world_has_grass_top() {
        let generator = FlatWorldGenerator;
        let mut chunk = ChunkColumn::new(0, 0);
        generator.generate(&mut chunk);
        assert_eq!(chunk.get_block(0, -61, 0), block::GRASS_BLOCK);
    }

    #[test]
    fn flat_world_air_above_grass() {
        let generator = FlatWorldGenerator;
        let mut chunk = ChunkColumn::new(0, 0);
        generator.generate(&mut chunk);
        assert_eq!(chunk.get_block(0, -60, 0), block::AIR);
        assert_eq!(chunk.get_block(0, 100, 0), block::AIR);
    }

    #[test]
    fn flat_world_all_columns_identical() {
        let generator = FlatWorldGenerator;
        let mut chunk = ChunkColumn::new(5, -3);
        generator.generate(&mut chunk);
        for x in 0..16 {
            for z in 0..16 {
                assert_eq!(chunk.get_block(x, -64, z), block::BEDROCK);
                assert_eq!(chunk.get_block(x, -61, z), block::GRASS_BLOCK);
            }
        }
    }
}
