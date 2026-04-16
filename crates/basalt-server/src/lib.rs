//! Basalt Minecraft server.
//!
//! A lightweight Minecraft 1.21.4 server built on the Basalt protocol
//! library. Uses a two-loop architecture: a network loop (movement,
//! chat, commands) and a game loop (blocks, world mutations) on
//! dedicated OS threads, connected by MPSC channels. Each incoming
//! TCP connection spawns a net task that fans packets out to the loops.
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

mod channels;
pub mod config;
mod connection;
pub mod error;
mod game_loop;
mod helpers;
mod io_thread;
mod messages;
mod net_task;
mod network_loop;
mod skin;
mod state;
mod tick;

use std::sync::Arc;

use config::ServerConfig;
use tokio::net::TcpListener;

use channels::LoopChannels;
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
    /// Creates the two-loop architecture with dedicated OS threads,
    /// an I/O thread for async persistence, and crash isolation.
    /// This method never returns under normal operation.
    pub async fn run(&self) {
        self.config.init_logger();

        let listener = match TcpListener::bind(&self.config.server.bind).await {
            Ok(l) => l,
            Err(e) => {
                log::error!(target: "basalt::server", "Failed to bind {}: {e}", self.config.server.bind);
                return;
            }
        };
        log::info!(target: "basalt::server", "Listening on {}", self.config.server.bind);

        self.run_with_listener(listener).await;
    }

    /// Accepts connections on the given listener with default config.
    ///
    /// Exposed for testing — tests can bind to port 0 and pass the
    /// listener directly, avoiding port conflicts.
    pub async fn accept_loop(listener: TcpListener) {
        let config = ServerConfig::default();
        let server = Server::with_config(config);
        server.run_with_listener(listener).await;
    }

    /// Core server loop: creates loops, I/O thread, and accepts connections.
    async fn run_with_listener(&self, listener: TcpListener) {
        let world = Arc::new(self.config.create_world());
        let plugins = self.config.create_plugins();
        let (state, network_bus, game_bus) =
            ServerState::build_for_loops(Arc::clone(&world), plugins);

        let channels = LoopChannels::new();
        let tps = self.config.server.tick_rate;
        let crash_on_panic = self.config.server.crash_on_plugin_panic;

        // Network loop — dedicated OS thread, guaranteed 20 TPS
        let mut net_loop = network_loop::NetworkLoop::new(
            network_bus,
            Arc::clone(&world),
            state.declare_commands.clone(),
            state.command_args.clone(),
            channels.network_rx,
        );
        // Tick loops are stopped by their Drop impl when this function returns.
        let _network_loop = tick::TickLoop::start("network-loop", tps, move |tick| {
            if crash_on_panic {
                net_loop.tick(tick);
            } else if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                net_loop.tick(tick);
            }))
            .is_err()
            {
                log::error!(target: "basalt::server", "Network loop tick {tick} panicked — continuing (crash_on_plugin_panic = false)");
            }
        });

        // I/O thread — dedicated OS thread for async chunk persistence
        let io_thread = io_thread::IoThread::start(Arc::clone(&world));

        // Game loop — dedicated OS thread, 20 TPS target
        // ECS with core components registered
        let mut ecs = basalt_ecs::Ecs::new();
        ecs.register_component::<basalt_ecs::Position>();
        ecs.register_component::<basalt_ecs::Rotation>();
        ecs.register_component::<basalt_ecs::Velocity>();
        ecs.register_component::<basalt_ecs::BoundingBox>();
        ecs.register_component::<basalt_ecs::EntityKind>();
        ecs.register_component::<basalt_ecs::Health>();
        ecs.register_component::<basalt_ecs::Lifetime>();
        ecs.register_component::<basalt_ecs::PlayerRef>();

        let mut game_loop_inst = game_loop::GameLoop::new(
            game_bus,
            Arc::clone(&world),
            channels.game_rx,
            io_thread.sender(),
            ecs,
        );
        let _game_loop = tick::TickLoop::start("game-loop", tps, move |tick| {
            if crash_on_panic {
                game_loop_inst.tick(tick);
            } else if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                game_loop_inst.tick(tick);
            }))
            .is_err()
            {
                log::error!(target: "basalt::server", "Game loop tick {tick} panicked — continuing (crash_on_plugin_panic = false)");
            }
        });

        log::info!(target: "basalt::server", "Network loop, game loop, and I/O thread started at {tps} TPS");

        // Accept connections and spawn net tasks
        let network_tx = channels.network_tx;
        let game_tx = channels.game_tx;
        loop {
            let (stream, addr) = match listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    log::error!(target: "basalt::connection", "Accept failed: {e}");
                    continue;
                }
            };
            log::debug!(target: "basalt::connection", "[{addr}] Accepted");

            let state = Arc::clone(&state);
            let network_tx = network_tx.clone();
            let game_tx = game_tx.clone();
            tokio::spawn(async move {
                if let Err(e) =
                    connection::handle_connection(stream, addr, state, network_tx, game_tx).await
                {
                    log::error!(target: "basalt::connection", "[{addr}] {e}");
                }
                log::debug!(target: "basalt::connection", "[{addr}] Closed");
            });
        }
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}
