//! Noise-based terrain generator.
//!
//! Uses layered Perlin noise to create natural-looking terrain with
//! hills, valleys, beaches, and underwater areas. The terrain has
//! biome-like variation based on altitude:
//!
//! - Deep underwater: gravel floor
//! - Shallow water: sand floor
//! - Beach (near sea level): sand
//! - Plains: grass + dirt
//! - Hills: grass + stone
//! - Mountains: stone + snow caps

use noise::{NoiseFn, Perlin};

use crate::block;
use crate::chunk::ChunkColumn;

/// Sea level Y coordinate — water fills up to this level.
const SEA_LEVEL: i32 = 62;

/// The base terrain height before noise is applied.
const BASE_HEIGHT: f64 = 64.0;

/// Maximum height variation from noise (±).
const HEIGHT_AMPLITUDE: f64 = 32.0;

/// Noise frequency — lower values produce smoother, larger features.
const FREQUENCY: f64 = 0.01;

/// Secondary noise frequency for detail variation.
const DETAIL_FREQUENCY: f64 = 0.05;

/// Amplitude of the detail noise layer.
const DETAIL_AMPLITUDE: f64 = 8.0;

/// Generates terrain using layered Perlin noise.
///
/// The generator uses two noise layers:
/// 1. A low-frequency layer for large terrain features (hills, valleys)
/// 2. A high-frequency layer for small surface detail
///
/// Block placement depends on altitude relative to sea level:
/// - Below sea level: water fills empty space, gravel/sand on the floor
/// - At sea level ±2: sand (beaches)
/// - Above sea level: grass on top, dirt underneath
/// - High altitude (>90): stone with snow caps
pub struct NoiseTerrainGenerator {
    /// Primary terrain shape noise.
    terrain_noise: Perlin,
    /// Secondary detail noise for surface variation.
    detail_noise: Perlin,
}

impl NoiseTerrainGenerator {
    /// Creates a new noise generator with the given seed.
    pub fn new(seed: u32) -> Self {
        Self {
            terrain_noise: Perlin::new(seed),
            detail_noise: Perlin::new(seed.wrapping_add(1)),
        }
    }

    /// The Y coordinate where players should spawn (above sea level).
    pub const SPAWN_Y: i32 = 80;

    /// Computes the terrain height at the given world (x, z) coordinates.
    fn height_at(&self, world_x: i32, world_z: i32) -> i32 {
        let x = world_x as f64;
        let z = world_z as f64;

        // Large-scale terrain shape
        let base = self.terrain_noise.get([x * FREQUENCY, z * FREQUENCY]);
        // Small-scale surface detail
        let detail = self
            .detail_noise
            .get([x * DETAIL_FREQUENCY, z * DETAIL_FREQUENCY]);

        let height = BASE_HEIGHT + base * HEIGHT_AMPLITUDE + detail * DETAIL_AMPLITUDE;
        height as i32
    }

    /// Populates a chunk column with noise-based terrain.
    pub fn generate(&self, chunk: &mut ChunkColumn) {
        for local_x in 0..16 {
            for local_z in 0..16 {
                let world_x = chunk.x * 16 + local_x as i32;
                let world_z = chunk.z * 16 + local_z as i32;
                let height = self.height_at(world_x, world_z);

                self.generate_column(chunk, local_x, local_z, height);
            }
        }
    }

    /// Fills a single (x, z) column from bedrock to the surface.
    fn generate_column(&self, chunk: &mut ChunkColumn, x: usize, z: usize, surface_y: i32) {
        // Bedrock at the very bottom
        chunk.set_block(x, -64, z, block::BEDROCK);

        // Stone from -63 up to a few blocks below surface
        for y in -63..=(surface_y - 4) {
            chunk.set_block(x, y, z, block::STONE);
        }

        // Surface layers depend on altitude
        if surface_y < SEA_LEVEL - 3 {
            // Deep underwater — gravel floor
            for y in (surface_y - 3)..=surface_y {
                chunk.set_block(x, y, z, block::GRAVEL);
            }
        } else if surface_y < SEA_LEVEL {
            // Shallow underwater or beach — sand
            for y in (surface_y - 3)..=surface_y {
                chunk.set_block(x, y, z, block::SAND);
            }
        } else if surface_y < SEA_LEVEL + 3 {
            // Beach area — sand on top
            for y in (surface_y - 3)..surface_y {
                chunk.set_block(x, y, z, block::SAND);
            }
            chunk.set_block(x, surface_y, z, block::SAND);
        } else if surface_y > 90 {
            // High altitude — stone with snow cap
            for y in (surface_y - 3)..surface_y {
                chunk.set_block(x, y, z, block::STONE);
            }
            chunk.set_block(x, surface_y, z, block::SNOW_BLOCK);
        } else {
            // Normal terrain — dirt with grass on top
            for y in (surface_y - 3)..surface_y {
                chunk.set_block(x, y, z, block::DIRT);
            }
            chunk.set_block(x, surface_y, z, block::GRASS_BLOCK);
        }

        // Fill water from surface up to sea level
        if surface_y < SEA_LEVEL {
            for y in (surface_y + 1)..=SEA_LEVEL {
                chunk.set_block(x, y, z, block::WATER);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_terrain_with_variation() {
        let generator = NoiseTerrainGenerator::new(42);
        let mut chunk = ChunkColumn::new(0, 0);
        generator.generate(&mut chunk);

        // Should have bedrock at bottom
        assert_eq!(chunk.get_block(0, -64, 0), block::BEDROCK);

        // Should have some non-air blocks above bedrock
        let mut has_surface = false;
        for y in -63..128 {
            if chunk.get_block(8, y, 8) != block::AIR && chunk.get_block(8, y, 8) != block::WATER {
                has_surface = true;
                break;
            }
        }
        assert!(has_surface, "terrain should have solid blocks");
    }

    #[test]
    fn same_seed_same_terrain() {
        let gen1 = NoiseTerrainGenerator::new(123);
        let gen2 = NoiseTerrainGenerator::new(123);

        let mut chunk1 = ChunkColumn::new(5, -3);
        let mut chunk2 = ChunkColumn::new(5, -3);
        gen1.generate(&mut chunk1);
        gen2.generate(&mut chunk2);

        // Same seed + same coords = same terrain
        for x in 0..16 {
            for z in 0..16 {
                for y in -64..128 {
                    assert_eq!(
                        chunk1.get_block(x, y, z),
                        chunk2.get_block(x, y, z),
                        "mismatch at ({x}, {y}, {z})"
                    );
                }
            }
        }
    }

    #[test]
    fn different_seeds_different_terrain() {
        let gen1 = NoiseTerrainGenerator::new(1);
        let gen2 = NoiseTerrainGenerator::new(999);

        // Check multiple points — at least one should differ
        let mut any_different = false;
        for x in 0..50 {
            if gen1.height_at(x * 16, 0) != gen2.height_at(x * 16, 0) {
                any_different = true;
                break;
            }
        }
        assert!(
            any_different,
            "different seeds should produce different terrain"
        );
    }

    #[test]
    fn height_varies_across_terrain() {
        let generator = NoiseTerrainGenerator::new(42);

        let mut heights = Vec::new();
        for x in 0..100 {
            heights.push(generator.height_at(x * 16, 0));
        }

        let min = *heights.iter().min().unwrap();
        let max = *heights.iter().max().unwrap();

        // Terrain should have at least some variation
        assert!(max - min > 5, "terrain should vary: min={min}, max={max}");
    }

    #[test]
    fn underwater_has_water() {
        let generator = NoiseTerrainGenerator::new(42);

        // Find a spot that's below sea level by checking many positions
        let mut found_water = false;
        for x in 0..200 {
            let h = generator.height_at(x, 0);
            if h < SEA_LEVEL {
                let mut chunk = ChunkColumn::new(x / 16, 0);
                generator.generate(&mut chunk);
                let local_x = (x % 16) as usize;
                // Water should be between surface and sea level
                if chunk.get_block(local_x, SEA_LEVEL, 0) == block::WATER {
                    found_water = true;
                    break;
                }
            }
        }
        assert!(found_water, "should find water somewhere below sea level");
    }
}
