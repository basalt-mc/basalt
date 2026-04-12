//! Player state tracking.
//!
//! Maintains the server-side state for a connected player: position,
//! rotation, gamemode, inventory, and keep-alive tracking. Updated by
//! the play loop as packets arrive from the client.

use std::collections::HashSet;
use std::time::Instant;

use basalt_api::ProfileProperty;
use basalt_types::{Slot, Uuid};

/// Number of hotbar slots in the player's inventory.
const HOTBAR_SIZE: usize = 9;

/// Server-side state for a connected player.
///
/// Created when the player enters the Play state and updated
/// continuously as the client sends movement and status packets.
pub(crate) struct PlayerState {
    /// The player's display name.
    pub username: String,
    /// The player's UUID (offline-mode generated or Mojang-assigned).
    pub uuid: Uuid,
    /// The player's entity ID in the world.
    pub entity_id: i32,
    /// Mojang profile properties (skin textures).
    pub skin_properties: Vec<ProfileProperty>,
    /// Current X coordinate in the world.
    pub x: f64,
    /// Current Y coordinate in the world.
    pub y: f64,
    /// Current Z coordinate in the world.
    pub z: f64,
    /// Current yaw rotation (horizontal look angle, degrees).
    pub yaw: f32,
    /// Current pitch rotation (vertical look angle, degrees).
    pub pitch: f32,
    /// Whether the player is on the ground.
    pub on_ground: bool,
    /// The last keep-alive ID sent to this player.
    pub last_keep_alive_id: i64,
    /// When the last keep-alive was sent, for RTT measurement.
    pub last_keep_alive_sent: Instant,
    /// Whether the player has confirmed the initial teleport.
    pub teleport_confirmed: bool,
    /// Whether the player has finished loading chunks.
    pub loaded: bool,
    /// Set of chunk coordinates that have been sent to this player.
    /// Used to avoid resending chunks the client already has.
    pub loaded_chunks: HashSet<(i32, i32)>,
    /// Currently selected hotbar slot (0-8).
    pub held_slot: u8,
    /// The 9 hotbar slots. Updated via `SetCreativeSlot` packets.
    pub hotbar: [Slot; HOTBAR_SIZE],
}

impl PlayerState {
    /// Creates a new player state with default spawn position.
    pub fn new(
        username: String,
        uuid: Uuid,
        entity_id: i32,
        skin_properties: Vec<ProfileProperty>,
    ) -> Self {
        Self {
            username,
            uuid,
            entity_id,
            skin_properties,
            x: 0.0,
            y: basalt_world::NoiseTerrainGenerator::SPAWN_Y as f64,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: false,
            last_keep_alive_id: 0,
            last_keep_alive_sent: Instant::now(),
            teleport_confirmed: false,
            loaded: false,
            loaded_chunks: HashSet::new(),
            held_slot: 0,
            hotbar: std::array::from_fn(|_| Slot::empty()),
        }
    }

    /// Updates the player's position from a movement packet.
    pub fn update_position(&mut self, x: f64, y: f64, z: f64) {
        self.x = x;
        self.y = y;
        self.z = z;
    }

    /// Updates the player's look direction from a movement packet.
    pub fn update_look(&mut self, yaw: f32, pitch: f32) {
        self.yaw = yaw;
        self.pitch = pitch;
    }

    /// Updates the on_ground flag from movement packet flags.
    ///
    /// The flags byte from movement packets has the on_ground bit
    /// at position 0 (LSB).
    pub fn update_on_ground(&mut self, flags: u8) {
        self.on_ground = flags & 0x01 != 0;
    }

    /// Returns the item the player is currently holding, if any.
    ///
    /// Looks up the held slot index in the hotbar array.
    pub fn held_item(&self) -> &Slot {
        &self.hotbar[self.held_slot as usize]
    }

    /// Sets a hotbar slot from a creative inventory packet.
    ///
    /// Creative mode slot indices 36-44 map to hotbar slots 0-8.
    /// Other slot indices are ignored (they refer to non-hotbar
    /// inventory slots which we don't track).
    pub fn set_creative_slot(&mut self, slot_index: i16, item: Slot) {
        if (36..=44).contains(&slot_index) {
            self.hotbar[(slot_index - 36) as usize] = item;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_player() -> PlayerState {
        PlayerState::new("Steve".into(), Uuid::default(), 1, vec![])
    }

    #[test]
    fn new_player_has_default_spawn() {
        let p = test_player();
        assert_eq!(p.x, 0.0);
        assert_eq!(p.y, basalt_world::NoiseTerrainGenerator::SPAWN_Y as f64);
        assert_eq!(p.z, 0.0);
        assert_eq!(p.yaw, 0.0);
        assert_eq!(p.pitch, 0.0);
        assert!(!p.on_ground);
        assert!(!p.teleport_confirmed);
        assert!(!p.loaded);
        assert_eq!(p.entity_id, 1);
        assert_eq!(p.username, "Steve");
    }

    #[test]
    fn update_position() {
        let mut p = test_player();
        p.update_position(10.5, 64.0, -30.2);
        assert_eq!(p.x, 10.5);
        assert_eq!(p.y, 64.0);
        assert_eq!(p.z, -30.2);
    }

    #[test]
    fn update_look() {
        let mut p = test_player();
        p.update_look(90.0, -45.0);
        assert_eq!(p.yaw, 90.0);
        assert_eq!(p.pitch, -45.0);
    }

    #[test]
    fn update_on_ground_flag() {
        let mut p = test_player();
        assert!(!p.on_ground);
        p.update_on_ground(0x01);
        assert!(p.on_ground);
        p.update_on_ground(0x00);
        assert!(!p.on_ground);
        // Other bits set but not the ground bit
        p.update_on_ground(0xFE);
        assert!(!p.on_ground);
        // Ground bit + other bits
        p.update_on_ground(0xFF);
        assert!(p.on_ground);
    }

    #[test]
    fn new_player_has_empty_hotbar() {
        let p = test_player();
        assert_eq!(p.held_slot, 0);
        assert!(p.held_item().is_empty());
        for slot in &p.hotbar {
            assert!(slot.is_empty());
        }
    }

    #[test]
    fn held_item_reflects_active_slot() {
        let mut p = test_player();
        p.hotbar[2] = Slot::new(1, 64); // stone × 64
        p.held_slot = 2;
        assert_eq!(p.held_item().item_id, Some(1));
        assert_eq!(p.held_item().item_count, 64);
    }

    #[test]
    fn set_creative_slot_updates_hotbar() {
        let mut p = test_player();
        p.set_creative_slot(36, Slot::new(1, 1)); // hotbar slot 0
        p.set_creative_slot(44, Slot::new(10, 32)); // hotbar slot 8
        assert_eq!(p.hotbar[0].item_id, Some(1));
        assert_eq!(p.hotbar[8].item_id, Some(10));
    }

    #[test]
    fn set_creative_slot_ignores_non_hotbar() {
        let mut p = test_player();
        p.set_creative_slot(0, Slot::new(1, 1));
        p.set_creative_slot(35, Slot::new(1, 1));
        p.set_creative_slot(45, Slot::new(1, 1));
        // All hotbar slots should still be empty
        for slot in &p.hotbar {
            assert!(slot.is_empty());
        }
    }
}
