//! Message types for net task → game loop communication.
//!
//! Net tasks forward game-relevant packets to the game loop via a
//! shared MPSC channel. Instant events (chat, commands) are handled
//! directly in the net task and never reach the game loop.

use basalt_core::broadcast::ProfileProperty;
use basalt_types::{Slot, Uuid};
use tokio::sync::mpsc;

/// Messages from net tasks to the game loop.
///
/// All net tasks share a single unbounded sender. The game loop
/// drains its receiver each tick via `try_recv()`.
pub enum GameInput {
    /// A new player has entered the Play state.
    ///
    /// The game loop spawns an ECS entity with all player components
    /// and sends the initial world data (Login, chunks, position).
    PlayerConnected {
        /// Server-assigned entity ID.
        entity_id: i32,
        /// Player UUID.
        uuid: Uuid,
        /// Player display name.
        username: String,
        /// Mojang skin texture data.
        skin_properties: Vec<ProfileProperty>,
        /// Initial spawn position.
        position: (f64, f64, f64),
        /// Initial yaw rotation.
        yaw: f32,
        /// Initial pitch rotation.
        pitch: f32,
        /// Channel for sending output packets to this player's net task.
        output_tx: mpsc::Sender<ServerOutput>,
    },
    /// A player has disconnected.
    PlayerDisconnected {
        /// UUID of the leaving player.
        uuid: Uuid,
    },
    /// Player position update.
    Position {
        /// UUID of the moving player.
        uuid: Uuid,
        /// New X coordinate.
        x: f64,
        /// New Y coordinate.
        y: f64,
        /// New Z coordinate.
        z: f64,
        /// Whether the player is on the ground.
        on_ground: bool,
    },
    /// Player look update.
    Look {
        /// UUID of the looking player.
        uuid: Uuid,
        /// New yaw angle (degrees).
        yaw: f32,
        /// New pitch angle (degrees).
        pitch: f32,
        /// Whether the player is on the ground.
        on_ground: bool,
    },
    /// Player position and look update.
    PositionLook {
        /// UUID of the moving player.
        uuid: Uuid,
        /// New X coordinate.
        x: f64,
        /// New Y coordinate.
        y: f64,
        /// New Z coordinate.
        z: f64,
        /// New yaw angle (degrees).
        yaw: f32,
        /// New pitch angle (degrees).
        pitch: f32,
        /// Whether the player is on the ground.
        on_ground: bool,
    },
    /// Block dig (status 0 = instant break in creative).
    BlockDig {
        /// UUID of the digging player.
        uuid: Uuid,
        /// Dig status.
        status: i32,
        /// Block X coordinate.
        x: i32,
        /// Block Y coordinate.
        y: i32,
        /// Block Z coordinate.
        z: i32,
        /// Sequence number for client acknowledgement.
        sequence: i32,
    },
    /// Block place.
    BlockPlace {
        /// UUID of the placing player.
        uuid: Uuid,
        /// Target block X coordinate.
        x: i32,
        /// Target block Y coordinate.
        y: i32,
        /// Target block Z coordinate.
        z: i32,
        /// Face direction (0-5).
        direction: i32,
        /// Sequence number for client acknowledgement.
        sequence: i32,
    },
    /// Player changed their held item slot.
    HeldItemSlot {
        /// UUID of the player.
        uuid: Uuid,
        /// New selected hotbar slot (0-8).
        slot: i16,
    },
    /// Player set a creative inventory slot.
    SetCreativeSlot {
        /// UUID of the player.
        uuid: Uuid,
        /// Inventory slot index.
        slot: i16,
        /// The item to place in the slot.
        item: Slot,
    },
}

/// Output from the game loop to a player's net task.
///
/// Each player has a dedicated bounded channel. The net task reads
/// from it and writes the encoded packets to the TCP connection.
#[derive(Clone, Debug)]
pub enum ServerOutput {
    /// A pre-encoded packet to send to the client.
    SendPacket {
        /// Minecraft packet ID.
        id: i32,
        /// Encoded packet payload (without the packet ID).
        data: Vec<u8>,
    },
}
