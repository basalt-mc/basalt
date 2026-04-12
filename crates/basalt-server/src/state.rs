//! Shared server state for multi-player coordination.
//!
//! `ServerState` is the central shared state passed as `Arc<ServerState>`
//! to each connection task. It holds the player registry (who's online),
//! an atomic entity ID counter, and provides methods for broadcasting
//! messages to all connected players.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use basalt_types::Uuid;
use basalt_types::nbt::NbtCompound;
use tokio::sync::{RwLock, mpsc};

/// Shared server state, held behind `Arc` and passed to every connection task.
pub(crate) struct ServerState {
    /// Atomic counter for assigning unique entity IDs.
    next_entity_id: AtomicI32,
    /// Registry of all connected players, keyed by UUID.
    players: RwLock<HashMap<Uuid, PlayerHandle>>,
}

/// A handle to a connected player, stored in the server state registry.
///
/// Contains the player's identity info and a channel sender for
/// delivering broadcast messages to their connection task.
#[derive(Debug)]
pub(crate) struct PlayerHandle {
    /// The player's display name.
    pub username: String,
    /// The player's UUID.
    pub uuid: Uuid,
    /// The player's unique entity ID.
    pub entity_id: i32,
    /// Channel for sending broadcast messages to this player's task.
    pub sender: mpsc::Sender<BroadcastMessage>,
}

/// A snapshot of a player's state at a point in time.
///
/// Used in broadcast messages to share a player's position and
/// identity with other connected players without holding locks.
#[derive(Debug, Clone)]
pub(crate) struct PlayerSnapshot {
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
}

/// A message broadcast from one player's task to all others.
///
/// Sent through the `mpsc` channel stored in each `PlayerHandle`.
/// The receiving task translates these into the appropriate
/// clientbound packets.
#[derive(Debug, Clone)]
pub(crate) enum BroadcastMessage {
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
}

impl ServerState {
    /// Creates a new empty server state.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            next_entity_id: AtomicI32::new(1),
            players: RwLock::new(HashMap::new()),
        })
    }

    /// Allocates a unique entity ID for a new player.
    pub fn next_entity_id(&self) -> i32 {
        self.next_entity_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Registers a player in the server state.
    ///
    /// Returns snapshots of all players who were already connected
    /// (the new player needs to know about them).
    pub async fn register_player(&self, handle: PlayerHandle) -> Vec<PlayerSnapshot> {
        let mut players = self.players.write().await;
        let existing: Vec<PlayerSnapshot> = players
            .values()
            .map(|h| PlayerSnapshot {
                username: h.username.clone(),
                uuid: h.uuid,
                entity_id: h.entity_id,
                // Position is not tracked in the handle — the joining
                // player will receive SpawnEntity with default coords.
                // Movement broadcasts update position in real time.
                x: 0.0,
                y: 100.0,
                z: 0.0,
                yaw: 0.0,
                pitch: 0.0,
            })
            .collect();
        players.insert(handle.uuid, handle);
        existing
    }

    /// Removes a player from the server state.
    pub async fn unregister_player(&self, uuid: &Uuid) {
        self.players.write().await.remove(uuid);
    }

    /// Broadcasts a message to all connected players.
    ///
    /// Sends the message to every player's channel. Players whose
    /// channel is full or closed are silently skipped (they will
    /// disconnect on their own).
    pub async fn broadcast(&self, message: BroadcastMessage) {
        let players = self.players.read().await;
        for handle in players.values() {
            let _ = handle.sender.try_send(message.clone());
        }
    }

    /// Broadcasts a message to all connected players except one.
    ///
    /// Used for movement updates where the moving player should
    /// not receive their own entity movement packet.
    pub async fn broadcast_except(&self, message: BroadcastMessage, except: &Uuid) {
        let players = self.players.read().await;
        for handle in players.values() {
            if &handle.uuid != except {
                let _ = handle.sender.try_send(message.clone());
            }
        }
    }

    /// Returns the number of currently connected players.
    #[cfg(test)]
    pub async fn player_count(&self) -> usize {
        self.players.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn next_entity_id_increments() {
        let state = ServerState::new();
        assert_eq!(state.next_entity_id(), 1);
        assert_eq!(state.next_entity_id(), 2);
        assert_eq!(state.next_entity_id(), 3);
    }

    #[tokio::test]
    async fn register_and_unregister_player() {
        let state = ServerState::new();
        let (tx, _rx) = mpsc::channel(16);

        let uuid = Uuid::default();
        let existing = state
            .register_player(PlayerHandle {
                username: "Steve".into(),
                uuid,
                entity_id: 1,
                sender: tx,
            })
            .await;

        assert!(existing.is_empty());
        assert_eq!(state.player_count().await, 1);

        state.unregister_player(&uuid).await;
        assert_eq!(state.player_count().await, 0);
    }

    #[tokio::test]
    async fn register_returns_existing_players() {
        let state = ServerState::new();
        let (tx1, _rx1) = mpsc::channel(16);
        let (tx2, _rx2) = mpsc::channel(16);

        let uuid1 = Uuid::from_bytes([1; 16]);
        let uuid2 = Uuid::from_bytes([2; 16]);

        state
            .register_player(PlayerHandle {
                username: "Alice".into(),
                uuid: uuid1,
                entity_id: 1,
                sender: tx1,
            })
            .await;

        let existing = state
            .register_player(PlayerHandle {
                username: "Bob".into(),
                uuid: uuid2,
                entity_id: 2,
                sender: tx2,
            })
            .await;

        assert_eq!(existing.len(), 1);
        assert_eq!(existing[0].username, "Alice");
        assert_eq!(state.player_count().await, 2);
    }

    #[tokio::test]
    async fn broadcast_sends_to_all() {
        let state = ServerState::new();
        let (tx1, mut rx1) = mpsc::channel(16);
        let (tx2, mut rx2) = mpsc::channel(16);

        let uuid1 = Uuid::from_bytes([1; 16]);
        let uuid2 = Uuid::from_bytes([2; 16]);

        state
            .register_player(PlayerHandle {
                username: "A".into(),
                uuid: uuid1,
                entity_id: 1,
                sender: tx1,
            })
            .await;
        state
            .register_player(PlayerHandle {
                username: "B".into(),
                uuid: uuid2,
                entity_id: 2,
                sender: tx2,
            })
            .await;

        state
            .broadcast(BroadcastMessage::Chat {
                content: NbtCompound::new(),
            })
            .await;

        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
    }

    #[tokio::test]
    async fn broadcast_except_skips_sender() {
        let state = ServerState::new();
        let (tx1, mut rx1) = mpsc::channel(16);
        let (tx2, mut rx2) = mpsc::channel(16);

        let uuid1 = Uuid::from_bytes([1; 16]);
        let uuid2 = Uuid::from_bytes([2; 16]);

        state
            .register_player(PlayerHandle {
                username: "A".into(),
                uuid: uuid1,
                entity_id: 1,
                sender: tx1,
            })
            .await;
        state
            .register_player(PlayerHandle {
                username: "B".into(),
                uuid: uuid2,
                entity_id: 2,
                sender: tx2,
            })
            .await;

        state
            .broadcast_except(
                BroadcastMessage::Chat {
                    content: NbtCompound::new(),
                },
                &uuid1,
            )
            .await;

        // A should NOT receive it
        assert!(rx1.try_recv().is_err());
        // B should receive it
        assert!(rx2.try_recv().is_ok());
    }
}
