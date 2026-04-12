//! Minimal Basalt server launcher.
//!
//! Starts a Minecraft 1.21.4 server on `localhost:25565` that accepts
//! clients into a void world. All server logic lives in the
//! `basalt-server` crate — this is just the entry point.
//!
//! Usage: `cargo run --package basalt-server --example server`

use basalt_server::Server;

#[tokio::main]
async fn main() {
    let server = Server::new("0.0.0.0:25565");
    server.run().await;
}
