//! Block state IDs for common Minecraft 1.21.4 blocks.
//!
//! These are the default state IDs from the vanilla data generator.
//! Each block can have multiple states (e.g., grass_block has snowy
//! and non-snowy variants), but we use the default (non-snowy) state.

/// Air — no block, fully transparent.
pub const AIR: u16 = 0;

/// Stone — basic underground block.
pub const STONE: u16 = 1;

/// Dirt — soil block without grass.
pub const DIRT: u16 = 10;

/// Grass block — dirt with grass on top (non-snowy variant).
pub const GRASS_BLOCK: u16 = 8;

/// Bedrock — indestructible bottom layer.
pub const BEDROCK: u16 = 85;

/// Water — still water (default state, level 0).
pub const WATER: u16 = 86;

/// Sand — beach and desert block.
pub const SAND: u16 = 118;

/// Gravel — underwater floor block.
pub const GRAVEL: u16 = 124;

/// Snow block — covers high-altitude terrain.
/// Snow block — covers high-altitude terrain tops.
pub const SNOW_BLOCK: u16 = 5950;
