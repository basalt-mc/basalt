//! Basalt Minecraft server.
//!
//! A lightweight Minecraft 1.21.4 server built on the Basalt protocol
//! library. Handles the full client lifecycle from handshake through
//! play, with support for multi-player, chat broadcast, commands,
//! and player position tracking.
//!
//! # Usage
//!
//! ```no_run
//! use basalt_server::Server;
//!
//! #[tokio::main]
//! async fn main() {
//!     let server = Server::new("0.0.0.0:25565");
//!     server.run().await;
//! }
//! ```

mod chat;
mod chunk;
mod connection;
mod helpers;
mod play;
mod player;
mod skin;
mod state;

use std::sync::Arc;

use tokio::net::TcpListener;

use state::ServerState;

/// A Basalt Minecraft server instance.
///
/// Listens for incoming TCP connections and spawns a task for each
/// client. All connection tasks share a `ServerState` that tracks
/// who is online, assigns unique entity IDs, and routes broadcast
/// messages between players.
pub struct Server {
    /// The address to bind the TCP listener to.
    bind_addr: String,
}

impl Server {
    /// Creates a new server that will listen on the given address.
    ///
    /// The address is not bound until `run()` is called. Use
    /// `"0.0.0.0:25565"` to listen on all interfaces on the default
    /// Minecraft port.
    pub fn new(bind_addr: &str) -> Self {
        Self {
            bind_addr: bind_addr.to_string(),
        }
    }

    /// Starts the server and listens for connections indefinitely.
    ///
    /// Each incoming connection is handled in its own Tokio task.
    /// This method never returns under normal operation.
    pub async fn run(&self) {
        let listener = TcpListener::bind(&self.bind_addr).await.unwrap();
        println!("Basalt server listening on {}", self.bind_addr);
        Self::accept_loop(listener).await;
    }

    /// Accepts connections on the given listener until it is dropped.
    ///
    /// Creates a shared `ServerState` and passes it to every connection
    /// task. Exposed for testing — tests can bind to port 0 and pass
    /// the listener directly, avoiding port conflicts.
    pub async fn accept_loop(listener: TcpListener) {
        let state = ServerState::new();

        loop {
            let (stream, addr) = listener.accept().await.unwrap();
            println!("[{addr}] Connection accepted");

            let state = Arc::clone(&state);
            tokio::spawn(async move {
                if let Err(e) = connection::handle_connection(stream, addr, state).await {
                    println!("[{addr}] Error: {e}");
                }
                println!("[{addr}] Connection closed");
            });
        }
    }
}
