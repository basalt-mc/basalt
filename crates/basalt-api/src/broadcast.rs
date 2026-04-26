//! Broadcast message types for cross-player communication.
//!
//! [`BroadcastMessage`] is the typed message sent through the server's
//! broadcast channel. Each connected player's play loop receives every
//! message and decides what to do with it (send packets, filter self).

use basalt_types::Uuid;
use basalt_types::nbt::NbtCompound;

/// A profile property from the Mojang API (typically skin textures).
///
/// Sent in the `PlayerInfo` packet's add_player action as part of the
/// game profile. The client uses the `textures` property to download
/// and render the player's skin.
#[derive(Debug, Clone)]
pub struct ProfileProperty {
    /// Property name (always "textures" for skins).
    pub name: String,
    /// Base64-encoded JSON containing the skin/cape URLs.
    pub value: String,
    /// Mojang signature for the property (base64-encoded).
    pub signature: Option<String>,
}

/// A snapshot of a player's state at a point in time.
///
/// Used in broadcast messages and events to share a player's position
/// and identity without holding locks on the player registry.
#[derive(Debug, Clone)]
pub struct PlayerSnapshot {
    /// The player's display name.
    pub username: String,
    /// The player's UUID.
    pub uuid: Uuid,
    /// The player's unique entity ID.
    pub entity_id: i32,
    /// Current X coordinate.
    pub x: f64,
    /// Current Y coordinate.
    pub y: f64,
    /// Current Z coordinate.
    pub z: f64,
    /// Current yaw (horizontal look angle, degrees).
    pub yaw: f32,
    /// Current pitch (vertical look angle, degrees).
    pub pitch: f32,
    /// Mojang profile properties (skin textures).
    pub skin_properties: Vec<ProfileProperty>,
}

/// A message broadcast from one player's task to all others.
///
/// Sent through the `broadcast::Sender` and received by each player's
/// `broadcast::Receiver` in their play loop. Plugins use
/// [`ChatContext::broadcast`](crate::context::ChatContext::broadcast)
/// to send these.
#[derive(Debug, Clone)]
pub enum BroadcastMessage {
    /// A chat message to display in all players' chat windows.
    Chat {
        /// The formatted text component as NBT.
        content: NbtCompound,
    },
    /// A new player has joined the server.
    PlayerJoined {
        /// Snapshot of the joining player's state.
        info: PlayerSnapshot,
    },
    /// A player has left the server.
    PlayerLeft {
        /// The leaving player's UUID (for PlayerRemove packet).
        uuid: Uuid,
        /// The leaving player's entity ID (for EntityDestroy packet).
        entity_id: i32,
        /// The leaving player's username (for chat message).
        username: String,
    },
    /// A player moved or changed look direction.
    EntityMoved {
        /// The moving player's entity ID.
        entity_id: i32,
        /// New absolute X coordinate.
        x: f64,
        /// New absolute Y coordinate.
        y: f64,
        /// New absolute Z coordinate.
        z: f64,
        /// New yaw angle (degrees).
        yaw: f32,
        /// New pitch angle (degrees).
        pitch: f32,
        /// Whether the player is on the ground.
        on_ground: bool,
    },
    /// A block was modified in the world.
    BlockChanged {
        /// Block X coordinate (absolute world coordinates).
        x: i32,
        /// Block Y coordinate (absolute world coordinates).
        y: i32,
        /// Block Z coordinate (absolute world coordinates).
        z: i32,
        /// The new block state ID.
        block_state: i32,
    },
}
