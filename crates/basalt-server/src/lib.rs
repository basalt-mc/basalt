//! Basalt Minecraft server.
//!
//! A lightweight Minecraft 1.21.4 server built on the Basalt protocol
//! library. Handles the full client lifecycle from handshake through
//! play, with plugin-based game logic loaded from `basalt.toml`.
//!
//! # Usage
//!
//! ```no_run
//! use basalt_server::Server;
//!
//! #[tokio::main]
//! async fn main() {
//!     let server = Server::new();
//!     server.run().await;
//! }
//! ```

mod chat;
pub mod config;
mod connection;
pub mod error;
mod helpers;
mod play;
mod player;
mod skin;
mod state;

use std::sync::Arc;

use config::ServerConfig;
use tokio::net::TcpListener;

use state::ServerState;

/// A Basalt Minecraft server instance.
///
/// Loads configuration from `basalt.toml` (or defaults), registers
/// plugins, and listens for incoming TCP connections.
pub struct Server {
    /// Server configuration loaded from `basalt.toml`.
    config: ServerConfig,
}

impl Server {
    /// Creates a new server with configuration loaded from `basalt.toml`.
    ///
    /// If `basalt.toml` doesn't exist, sensible defaults are used
    /// (all plugins enabled, read-write storage, seed 42).
    pub fn new() -> Self {
        Self {
            config: ServerConfig::load(),
        }
    }

    /// Creates a new server with the given configuration.
    pub fn with_config(config: ServerConfig) -> Self {
        Self { config }
    }

    /// Starts the server and listens for connections indefinitely.
    ///
    /// Each incoming connection is handled in its own Tokio task.
    /// This method never returns under normal operation.
    pub async fn run(&self) {
        let listener = TcpListener::bind(&self.config.server.bind).await.unwrap();
        println!("Basalt server listening on {}", self.config.server.bind);

        let world = self.config.create_world();
        let plugins = self.config.create_plugins();
        let state = ServerState::with_world_and_plugins(world, plugins);

        Self::accept_loop_with_state(listener, state).await;
    }

    /// Accepts connections on the given listener with default config.
    ///
    /// Exposed for testing — tests can bind to port 0 and pass the
    /// listener directly, avoiding port conflicts.
    pub async fn accept_loop(listener: TcpListener) {
        let config = ServerConfig::default();
        let world = config.create_world();
        let plugins = config.create_plugins();
        let state = ServerState::with_world_and_plugins(world, plugins);

        Self::accept_loop_with_state(listener, state).await;
    }

    /// Accepts connections with an existing server state.
    async fn accept_loop_with_state(listener: TcpListener, state: Arc<ServerState>) {
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

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}
