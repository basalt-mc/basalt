//! Channel infrastructure for the server architecture.
//!
//! Provides the MPSC channel from net tasks to the game loop, a
//! broadcast channel for instant fan-out (chat, commands), and a
//! shared player registry for targeted sending.

use std::sync::Arc;

use basalt_types::Uuid;
use dashmap::DashMap;
use tokio::sync::{broadcast, mpsc};

use crate::messages::{GameInput, ServerOutput};

/// Capacity of each player's output channel.
const PLAYER_OUTPUT_CAPACITY: usize = 256;

/// Capacity of the broadcast channel for instant messages.
const BROADCAST_CAPACITY: usize = 256;

/// All shared state needed by net tasks and the game loop.
pub(crate) struct SharedState {
    /// Sender for net tasks → game loop. Cloned per net task.
    pub game_tx: mpsc::UnboundedSender<GameInput>,
    /// Receiver for net tasks → game loop. Owned by the game loop.
    pub game_rx: mpsc::UnboundedReceiver<GameInput>,
    /// Broadcast sender for instant fan-out (chat, commands).
    /// Net tasks send here; all net tasks receive via subscription.
    pub broadcast_tx: broadcast::Sender<ServerOutput>,
    /// Shared registry of connected players' output channels.
    /// Used by instant event handlers for targeted sending.
    pub player_registry: Arc<DashMap<Uuid, mpsc::Sender<ServerOutput>>>,
}

impl SharedState {
    /// Creates all shared state for the server.
    pub fn new() -> Self {
        let (game_tx, game_rx) = mpsc::unbounded_channel();
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_CAPACITY);

        Self {
            game_tx,
            game_rx,
            broadcast_tx,
            player_registry: Arc::new(DashMap::new()),
        }
    }
}

/// Creates a bounded output channel for a single player.
pub(crate) fn player_output_channel() -> (mpsc::Sender<ServerOutput>, mpsc::Receiver<ServerOutput>)
{
    mpsc::channel(PLAYER_OUTPUT_CAPACITY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_state_creation() {
        let mut state = SharedState::new();
        state
            .game_tx
            .send(GameInput::PlayerDisconnected {
                uuid: Uuid::default(),
            })
            .unwrap();
        assert!(state.game_rx.try_recv().is_ok());
    }

    #[test]
    fn player_output_channel_bounded() {
        let (tx, mut rx) = player_output_channel();
        tx.try_send(ServerOutput::SendPacket {
            id: 0x00,
            data: vec![],
        })
        .unwrap();
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn broadcast_channel_delivers() {
        let state = SharedState::new();
        let mut rx = state.broadcast_tx.subscribe();
        state
            .broadcast_tx
            .send(ServerOutput::SendPacket {
                id: 0x01,
                data: vec![42],
            })
            .unwrap();
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn player_registry_insert_and_lookup() {
        let registry: Arc<DashMap<Uuid, mpsc::Sender<ServerOutput>>> = Arc::new(DashMap::new());
        let (tx, _rx) = player_output_channel();
        let uuid = Uuid::from_bytes([1; 16]);
        registry.insert(uuid, tx);
        assert!(registry.get(&uuid).is_some());
    }
}
