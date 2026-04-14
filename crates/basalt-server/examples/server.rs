//! Minimal Basalt server launcher.
//!
//! Loads configuration from `basalt.toml` (or uses defaults) and
//! starts a Minecraft 1.21.4 server. Plugins, bind address, world
//! seed, and storage mode are all configurable.
//!
//! Usage: `cargo run --package basalt-server --example server`

use basalt_server::Server;

#[tokio::main]
async fn main() {
    let server = Server::new();
    server.run().await;
}
