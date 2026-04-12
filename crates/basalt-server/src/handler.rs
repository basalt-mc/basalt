//! Packet handler trait for extensible packet dispatch.
//!
//! The `PacketHandler` trait decouples packet processing from the play
//! loop. Each handler implements `handle()` which receives the packet,
//! player state, and a `PacketContext` for sending responses. This
//! pattern is extensible for plugins — a plugin registers handlers
//! that the server calls alongside built-in ones.

use std::net::SocketAddr;
use std::sync::Arc;

use basalt_net::connection::{Connection, Play};

use crate::state::ServerState;

/// Context available to packet handlers for sending responses
/// and accessing shared state.
///
/// Wraps the connection and server state so handlers don't need
/// to take multiple parameters. Also carries the player's address
/// for logging.
pub(crate) struct PacketContext<'a> {
    /// The player's TCP connection for sending response packets.
    pub conn: &'a mut Connection<Play>,
    /// Shared server state for broadcasting and player lookups.
    pub state: &'a Arc<ServerState>,
    /// The player's socket address for logging.
    #[allow(dead_code)]
    pub addr: SocketAddr,
}
