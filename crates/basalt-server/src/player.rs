//! Player state tracking.
//!
//! Maintains the server-side state for a connected player: position,
//! rotation, gamemode, and keep-alive tracking. Updated by the play
//! loop as packets arrive from the client.

use std::time::Instant;

use basalt_types::Uuid;

use crate::skin::ProfileProperty;

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
            y: 100.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: false,
            last_keep_alive_id: 0,
            last_keep_alive_sent: Instant::now(),
            teleport_confirmed: false,
            loaded: false,
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
        assert_eq!(p.y, 100.0);
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
}
