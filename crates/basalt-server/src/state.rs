//! Shared server state for multi-player coordination.
//!
//! `ServerState` is the central shared state passed as `Arc<ServerState>`
//! to each connection task. It holds the player registry (who's online),
//! an atomic entity ID counter, and a broadcast channel for fan-out
//! messages to all connected players.

use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use basalt_types::Uuid;
use basalt_types::nbt::NbtCompound;
use dashmap::DashMap;
use tokio::sync::broadcast;

use crate::skin::ProfileProperty;

/// Shared server state, held behind `Arc` and passed to every connection task.
pub(crate) struct ServerState {
    /// Atomic counter for assigning unique entity IDs.
    next_entity_id: AtomicI32,
    /// Lock-free registry of all connected players, keyed by UUID.
    players: DashMap<Uuid, PlayerHandle>,
    /// Broadcast channel sender — O(1) fan-out to all subscribers.
    broadcast_tx: broadcast::Sender<BroadcastMessage>,
    /// The world — chunk cache and terrain generator.
    pub world: basalt_world::World,
}

/// A handle to a connected player, stored in the server state registry.
///
/// Contains the player's identity info. Broadcast messages are delivered
/// via the shared `broadcast::Sender` rather than per-player channels.
#[derive(Debug)]
pub(crate) struct PlayerHandle {
    /// The player's display name.
    pub username: String,
    /// The player's UUID.
    pub uuid: Uuid,
    /// The player's unique entity ID.
    pub entity_id: i32,
    /// Mojang profile properties (skin textures).
    pub skin_properties: Vec<ProfileProperty>,
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
    /// Mojang profile properties (skin textures).
    pub skin_properties: Vec<ProfileProperty>,
}

/// A message broadcast from one player's task to all others.
///
/// Sent through the `broadcast::Sender` and received by each player's
/// `broadcast::Receiver` in their play loop.
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

impl ServerState {
    /// Creates a new empty server state with a broadcast channel.
    pub fn new() -> Arc<Self> {
        // Buffer size for broadcast — receivers that fall behind lose
        // old messages. 256 is enough for ~5 seconds of movement
        // updates from 10 players at 20Hz.
        let (broadcast_tx, _) = broadcast::channel(256);
        Arc::new(Self {
            next_entity_id: AtomicI32::new(1),
            players: DashMap::new(),
            broadcast_tx,
            world: basalt_world::World::new(42, "world"),
        })
    }

    /// Allocates a unique entity ID for a new player.
    pub fn next_entity_id(&self) -> i32 {
        self.next_entity_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Subscribes to the broadcast channel. Each player task calls
    /// this once and polls the receiver in their play loop.
    pub fn subscribe(&self) -> broadcast::Receiver<BroadcastMessage> {
        self.broadcast_tx.subscribe()
    }

    /// Registers a player in the server state.
    ///
    /// Returns snapshots of all players who were already connected
    /// (the new player needs to know about them).
    pub fn register_player(&self, handle: PlayerHandle) -> Vec<PlayerSnapshot> {
        let existing: Vec<PlayerSnapshot> = self
            .players
            .iter()
            .map(|entry| {
                let h = entry.value();
                PlayerSnapshot {
                    username: h.username.clone(),
                    uuid: h.uuid,
                    entity_id: h.entity_id,
                    x: 0.0,
                    y: basalt_world::NoiseTerrainGenerator::SPAWN_Y as f64,
                    z: 0.0,
                    yaw: 0.0,
                    pitch: 0.0,
                    skin_properties: h.skin_properties.clone(),
                }
            })
            .collect();
        self.players.insert(handle.uuid, handle);
        existing
    }

    /// Removes a player from the server state.
    pub fn unregister_player(&self, uuid: &Uuid) {
        self.players.remove(uuid);
    }

    /// Broadcasts a message to all connected players.
    ///
    /// Uses the broadcast channel for O(1) fan-out. Receivers that
    /// have fallen behind will miss old messages (acceptable for
    /// movement updates, chat is rare enough to fit in the buffer).
    pub fn broadcast(&self, message: BroadcastMessage) {
        // Ignore send errors — they mean no receivers are listening.
        let _ = self.broadcast_tx.send(message);
    }

    /// Returns the number of currently connected players.
    #[cfg(test)]
    pub fn player_count(&self) -> usize {
        self.players.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_entity_id_increments() {
        let state = ServerState::new();
        assert_eq!(state.next_entity_id(), 1);
        assert_eq!(state.next_entity_id(), 2);
        assert_eq!(state.next_entity_id(), 3);
    }

    #[test]
    fn register_and_unregister_player() {
        let state = ServerState::new();

        let uuid = Uuid::default();
        let existing = state.register_player(PlayerHandle {
            username: "Steve".into(),
            uuid,
            entity_id: 1,
            skin_properties: vec![],
        });

        assert!(existing.is_empty());
        assert_eq!(state.player_count(), 1);

        state.unregister_player(&uuid);
        assert_eq!(state.player_count(), 0);
    }

    #[test]
    fn register_returns_existing_players() {
        let state = ServerState::new();

        let uuid1 = Uuid::from_bytes([1; 16]);
        let uuid2 = Uuid::from_bytes([2; 16]);

        state.register_player(PlayerHandle {
            username: "Alice".into(),
            uuid: uuid1,
            entity_id: 1,
            skin_properties: vec![],
        });

        let existing = state.register_player(PlayerHandle {
            username: "Bob".into(),
            uuid: uuid2,
            entity_id: 2,
            skin_properties: vec![],
        });

        assert_eq!(existing.len(), 1);
        assert_eq!(existing[0].username, "Alice");
        assert_eq!(state.player_count(), 2);
    }

    #[test]
    fn broadcast_delivers_to_subscriber() {
        let state = ServerState::new();
        let mut rx = state.subscribe();

        state.broadcast(BroadcastMessage::Chat {
            content: NbtCompound::new(),
        });

        let msg = rx.try_recv().unwrap();
        assert!(matches!(msg, BroadcastMessage::Chat { .. }));
    }

    #[test]
    fn broadcast_delivers_to_multiple_subscribers() {
        let state = ServerState::new();
        let mut rx1 = state.subscribe();
        let mut rx2 = state.subscribe();

        state.broadcast(BroadcastMessage::Chat {
            content: NbtCompound::new(),
        });

        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
    }
}
