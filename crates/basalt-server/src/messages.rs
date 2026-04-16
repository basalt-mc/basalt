//! Message types for inter-loop communication.
//!
//! The server's two-loop architecture uses MPSC channels to pass
//! messages between net tasks (TCP I/O), the network loop (movement,
//! chat, commands), and the game loop (blocks, world mutations).
//! Each enum represents the message vocabulary for one channel.

use basalt_core::broadcast::ProfileProperty;
use basalt_types::{Slot, Uuid};
use tokio::sync::mpsc;

/// Messages from net tasks to the network loop.
///
/// All net tasks share a single unbounded sender. The network loop
/// drains its receiver each tick via `try_recv()`.
pub enum NetworkInput {
    /// A new player has entered the Play state and needs network
    /// state initialization (position tracking, loaded chunks, etc.).
    PlayerConnected {
        /// Server-assigned entity ID.
        entity_id: i32,
        /// Player UUID (from Mojang or offline-mode).
        uuid: Uuid,
        /// Player display name.
        username: String,
        /// Mojang skin texture data.
        skin_properties: Vec<ProfileProperty>,
        /// Initial world position.
        position: (f64, f64, f64),
        /// Initial yaw rotation (horizontal, degrees).
        yaw: f32,
        /// Initial pitch rotation (vertical, degrees).
        pitch: f32,
        /// Channel for sending output packets back to this player.
        output_tx: mpsc::Sender<ServerOutput>,
    },
    /// A player has disconnected (timeout, error, or quit packet).
    PlayerDisconnected {
        /// UUID of the leaving player.
        uuid: Uuid,
        /// Entity ID of the leaving player.
        entity_id: i32,
        /// Username of the leaving player.
        username: String,
    },
    /// Player position update (from Position packet).
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
    /// Player look update (from Look packet).
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
    /// Player position and look update (from PositionLook packet).
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
    /// Chat message from a player.
    ChatMessage {
        /// UUID of the sender.
        uuid: Uuid,
        /// Sender's username (for display formatting).
        username: String,
        /// The message text.
        message: String,
    },
    /// Slash command from a player (without the leading `/`).
    ChatCommand {
        /// UUID of the command issuer.
        uuid: Uuid,
        /// The command string (e.g., `"tp 0 64 0"`).
        command: String,
    },
}

/// Messages from net tasks to the game loop.
///
/// All net tasks share a single unbounded sender. The game loop
/// drains its receiver each tick via `try_recv()`.
pub enum GameInput {
    /// A new player has entered the Play state and needs game
    /// state initialization (inventory, held item, etc.).
    PlayerConnected {
        /// Server-assigned entity ID.
        entity_id: i32,
        /// Player UUID.
        uuid: Uuid,
        /// Player display name.
        username: String,
        /// Initial spawn position.
        position: (f64, f64, f64),
        /// Channel for sending output packets back to this player.
        output_tx: mpsc::Sender<ServerOutput>,
    },
    /// A player has disconnected.
    PlayerDisconnected {
        /// UUID of the leaving player.
        uuid: Uuid,
    },
    /// Block dig (status 0 = started digging / instant break in creative).
    ///
    /// The game loop validates the action, mutates the world, and
    /// broadcasts the block change to all players.
    BlockDig {
        /// UUID of the digging player.
        uuid: Uuid,
        /// Dig status (0 = started/instant break in creative).
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
    /// Block place with full placement data.
    ///
    /// The game loop validates, computes the block state from the
    /// player's held item, mutates the world, and broadcasts.
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

/// Output from the loops to a player's net task.
///
/// Each player has a dedicated bounded channel. Both the network
/// loop and game loop hold a clone of the sender. The net task
/// reads from the receiver and writes to the TCP connection.
#[derive(Clone)]
pub enum ServerOutput {
    /// A pre-encoded packet to send to the client.
    ///
    /// The packet ID and payload have been encoded by the loop.
    /// The net task writes them through the [`Connection`]'s framing
    /// layer (length prefix, compression, encryption).
    SendPacket {
        /// Minecraft packet ID.
        id: i32,
        /// Encoded packet payload (without the packet ID).
        data: Vec<u8>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_input_position_construction() {
        let msg = NetworkInput::Position {
            uuid: Uuid::default(),
            x: 1.0,
            y: 64.0,
            z: -3.0,
            on_ground: true,
        };
        assert!(matches!(msg, NetworkInput::Position { x, .. } if x == 1.0));
    }

    #[test]
    fn game_input_block_dig_construction() {
        let msg = GameInput::BlockDig {
            uuid: Uuid::default(),
            status: 0,
            x: 5,
            y: 64,
            z: 3,
            sequence: 42,
        };
        assert!(matches!(msg, GameInput::BlockDig { sequence: 42, .. }));
    }

    #[test]
    fn server_output_send_packet_construction() {
        let msg = ServerOutput::SendPacket {
            id: 0x1A,
            data: vec![1, 2, 3],
        };
        assert!(matches!(msg, ServerOutput::SendPacket { id: 0x1A, .. }));
    }
}
