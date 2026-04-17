//! Basalt Minecraft server.
//!
//! A single game loop on a dedicated OS thread handles all tick-based
//! simulation (movement, blocks, physics, AI). Instant events (chat,
//! commands) are dispatched directly in per-player net tasks for zero
//! latency. Each TCP connection spawns a net task for I/O and packet
//! classification.
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

pub mod config;
pub mod error;
mod game;
mod helpers;
mod messages;
mod net;
mod runtime;
mod state;

use std::sync::Arc;

use config::ServerConfig;
use tokio::net::TcpListener;

use net::channels::SharedState;
use state::ServerState;

/// A Basalt Minecraft server instance.
pub struct Server {
    /// Server configuration loaded from `basalt.toml`.
    config: ServerConfig,
}

impl Server {
    /// Creates a new server with configuration loaded from `basalt.toml`.
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
    pub async fn accept_loop(listener: TcpListener) {
        let config = ServerConfig::default();
        let server = Server::with_config(config);
        server.run_with_listener(listener).await;
    }

    /// Core server loop: game loop + I/O thread + accept connections.
    async fn run_with_listener(&self, listener: TcpListener) {
        let world = Arc::new(self.config.create_world());
        let plugins = self.config.create_plugins();
        let (server_state, instant_bus, game_bus, plugin_systems, plugin_components) =
            ServerState::build_for_loops(Arc::clone(&world), plugins);

        let shared = SharedState::new();
        let tps = self.config.server.tick_rate;
        let crash_on_panic = self.config.server.crash_on_plugin_panic;

        // Wrap the instant bus in Arc for sharing across net tasks
        let instant_bus = Arc::new(instant_bus);

        // I/O thread — dedicated OS thread for async chunk persistence
        let io_thread = runtime::io_thread::IoThread::start(Arc::clone(&world));

        // ECS with core components
        let mut ecs = basalt_ecs::Ecs::new();
        ecs.register_component::<basalt_ecs::Position>();
        ecs.register_component::<basalt_ecs::Rotation>();
        ecs.register_component::<basalt_ecs::Velocity>();
        ecs.register_component::<basalt_ecs::BoundingBox>();
        ecs.register_component::<basalt_ecs::EntityKind>();
        ecs.register_component::<basalt_ecs::Health>();
        ecs.register_component::<basalt_ecs::Lifetime>();
        ecs.register_component::<basalt_ecs::PlayerRef>();
        ecs.register_component::<basalt_ecs::Inventory>();
        for reg in &plugin_components {
            (reg.register_fn)(&mut ecs);
        }
        for system in plugin_systems {
            ecs.add_system(system);
        }

        // Game loop — single dedicated OS thread
        let mut game_loop_inst = game::GameLoop::new(
            game_bus,
            Arc::clone(&world),
            shared.game_rx,
            io_thread.sender(),
            ecs,
            server_state.declare_commands.clone(),
        );
        let _game_loop = runtime::tick::TickLoop::start("game-loop", tps, move |tick| {
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

        log::info!(target: "basalt::server", "Game loop and I/O thread started at {tps} TPS");

        // Accept connections and spawn net tasks
        let game_tx = shared.game_tx;
        let broadcast_tx = shared.broadcast_tx;
        let player_registry = shared.player_registry;
        loop {
            let (stream, addr) = match listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    log::error!(target: "basalt::connection", "Accept failed: {e}");
                    continue;
                }
            };
            log::debug!(target: "basalt::connection", "[{addr}] Accepted");

            let server_state = Arc::clone(&server_state);
            let game_tx = game_tx.clone();
            let instant_bus = Arc::clone(&instant_bus);
            let broadcast_tx = broadcast_tx.clone();
            let player_registry = Arc::clone(&player_registry);
            let world = Arc::clone(&world);
            tokio::spawn(async move {
                if let Err(e) = net::connection::handle_connection(
                    stream,
                    addr,
                    server_state,
                    game_tx,
                    instant_bus,
                    broadcast_tx,
                    player_registry,
                    world,
                )
                .await
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
