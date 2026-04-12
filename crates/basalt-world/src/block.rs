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
