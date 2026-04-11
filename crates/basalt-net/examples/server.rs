//! Minimal Minecraft server that responds to ping and accepts login.
//!
//! - Status flow: responds with server info + ping
//! - Login flow: reads LoginStart, logs username, sends Disconnect
//!
//! Usage: `cargo run --package basalt-net --example server`
//! Then open Minecraft 1.21.x and add `localhost:25565` to the server list.

use basalt_net::connection::{Connection, Handshake, HandshakeResult};
use basalt_protocol::packets::login::ServerboundLoginPacket;
use basalt_protocol::packets::status::{
    ClientboundStatusPing, ClientboundStatusServerInfo, ServerboundStatusPacket,
};
use tokio::net::TcpListener;

const SERVER_STATUS: &str = r#"{
    "version": {
        "name": "Basalt 1.21",
        "protocol": 767
    },
    "players": {
        "max": 20,
        "online": 0,
        "sample": []
    },
    "description": {
        "text": "A Basalt Server"
    },
    "enforcesSecureChat": false
}"#;

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("0.0.0.0:25565").await.unwrap();
    println!("Basalt server listening on 0.0.0.0:25565");

    loop {
        let (stream, addr) = listener.accept().await.unwrap();
        println!("[{addr}] Connection accepted");

        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, addr).await {
                println!("[{addr}] Error: {e}");
            }
            println!("[{addr}] Connection closed");
        });
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    addr: std::net::SocketAddr,
) -> basalt_net::Result<()> {
    let conn = Connection::<Handshake>::accept(stream);

    match conn.read_handshake().await? {
        HandshakeResult::Status(conn, handshake) => {
            println!(
                "[{addr}] Status request (protocol {})",
                handshake.protocol_version
            );
            handle_status(conn, addr).await
        }
        HandshakeResult::Login(conn, handshake) => {
            println!(
                "[{addr}] Login request (protocol {})",
                handshake.protocol_version
            );
            handle_login(conn, addr).await
        }
    }
}

async fn handle_status(
    mut conn: Connection<basalt_net::connection::Status>,
    addr: std::net::SocketAddr,
) -> basalt_net::Result<()> {
    // Read StatusRequest
    let packet = conn.read_packet().await?;
    if let ServerboundStatusPacket::PingStart(_) = packet {
        println!("[{addr}] <- StatusRequest");
    }

    // Send StatusResponse
    let response = ClientboundStatusServerInfo {
        response: SERVER_STATUS.into(),
    };
    conn.write_status_response(&response).await?;
    println!("[{addr}] -> StatusResponse");

    // Read Ping
    let packet = conn.read_packet().await?;
    if let ServerboundStatusPacket::Ping(ping) = packet {
        println!("[{addr}] <- Ping (time={})", ping.time);
        let pong = ClientboundStatusPing { time: ping.time };
        conn.write_ping_response(&pong).await?;
        println!("[{addr}] -> Pong");
    }

    Ok(())
}

async fn handle_login(
    mut conn: Connection<basalt_net::connection::Login>,
    addr: std::net::SocketAddr,
) -> basalt_net::Result<()> {
    // Read LoginStart
    let packet = conn.read_packet().await?;
    match packet {
        ServerboundLoginPacket::LoginStart(login) => {
            println!(
                "[{addr}] <- LoginStart (username={}, uuid={})",
                login.username, login.player_uuid
            );
        }
        other => {
            println!("[{addr}] <- Unexpected login packet: {other:?}");
        }
    }

    // Send Disconnect
    let reason = r#"{"text":"Basalt server is not ready yet. Thanks for testing!"}"#;
    conn.disconnect(reason).await?;
    println!("[{addr}] -> Disconnect");

    Ok(())
}
