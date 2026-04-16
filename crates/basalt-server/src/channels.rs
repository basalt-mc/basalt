//! Channel infrastructure for the two-loop architecture.
//!
//! Creates and manages the MPSC channels that connect net tasks to the
//! network and game loops. Each loop has a single shared input channel
//! (all net tasks send to one receiver). Each player gets a dedicated
//! bounded output channel for backpressure detection.

use tokio::sync::mpsc;

use crate::messages::{GameInput, NetworkInput, ServerOutput};

/// Capacity of each player's output channel.
///
/// When the channel is full, the loop's `try_send` fails, indicating
/// the client is lagging. The server can then kick or throttle.
const PLAYER_OUTPUT_CAPACITY: usize = 256;

/// All channels needed by the two-loop architecture.
///
/// Created once at server startup. The senders are cloned to net tasks
/// and loops as needed; the receivers are owned by the respective loops.
pub(crate) struct LoopChannels {
    /// Sender for net tasks → network loop. Cloned per net task.
    pub network_tx: mpsc::UnboundedSender<NetworkInput>,
    /// Receiver for net tasks → network loop. Owned by the network loop.
    pub network_rx: mpsc::UnboundedReceiver<NetworkInput>,
    /// Sender for net tasks → game loop. Cloned per net task.
    pub game_tx: mpsc::UnboundedSender<GameInput>,
    /// Receiver for net tasks → game loop. Owned by the game loop.
    pub game_rx: mpsc::UnboundedReceiver<GameInput>,
}

impl LoopChannels {
    /// Creates all channels for the two-loop architecture.
    pub fn new() -> Self {
        let (network_tx, network_rx) = mpsc::unbounded_channel();
        let (game_tx, game_rx) = mpsc::unbounded_channel();

        Self {
            network_tx,
            network_rx,
            game_tx,
            game_rx,
        }
    }
}

/// Creates a bounded output channel for a single player.
///
/// Returns `(sender, receiver)`. The sender is cloned to both loops
/// (they both produce output for this player). The receiver is owned
/// by the player's net task, which relays packets to the TCP connection.
pub(crate) fn player_output_channel() -> (mpsc::Sender<ServerOutput>, mpsc::Receiver<ServerOutput>)
{
    mpsc::channel(PLAYER_OUTPUT_CAPACITY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_channels_creation() {
        let mut channels = LoopChannels::new();
        channels
            .network_tx
            .send(NetworkInput::PlayerDisconnected {
                uuid: basalt_types::Uuid::default(),
                entity_id: 1,
                username: "test".into(),
            })
            .unwrap();
        assert!(channels.network_rx.try_recv().is_ok());
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
}
