//! End-to-end tests simulating a real Minecraft client connecting to the server.
//!
//! Each test spawns a server task on a random port, then connects a fake client
//! that speaks the Minecraft protocol. This validates the full pipeline:
//! types → derive → protocol → framing → connection.

use basalt_mc_protocol::packets::handshake::ServerboundHandshakeSetProtocol;
use basalt_mc_protocol::packets::login::{
    ClientboundLoginDisconnect, ServerboundLoginLoginStart, ServerboundLoginPacket,
};
use basalt_mc_protocol::packets::status::{
    ClientboundStatusPing, ClientboundStatusServerInfo, ServerboundStatusPacket,
    ServerboundStatusPing, ServerboundStatusPingStart,
};
use basalt_net::connection::{Connection, Handshake, HandshakeResult};
use basalt_net::framing;
use basalt_types::{Decode, Encode, EncodedSize, Uuid};
use tokio::net::{TcpListener, TcpStream};

/// Sends a framed packet from the client side.
async fn send_packet<P: Encode + EncodedSize>(stream: &mut TcpStream, packet_id: i32, packet: &P) {
    let mut payload = Vec::with_capacity(packet.encoded_size());
    packet.encode(&mut payload).unwrap();
    framing::write_raw_packet(stream, packet_id, &payload)
        .await
        .unwrap();
}

/// Reads a framed packet from the client side and decodes it.
async fn recv_packet<P: Decode>(stream: &mut TcpStream) -> (i32, P) {
    let raw = framing::read_raw_packet(stream).await.unwrap().unwrap();
    let mut cursor = raw.payload.as_slice();
    let packet = P::decode(&mut cursor).unwrap();
    (raw.id, packet)
}

/// Sends a handshake packet from the client.
async fn client_handshake(stream: &mut TcpStream, next_state: i32) {
    let handshake = ServerboundHandshakeSetProtocol {
        protocol_version: 767,
        server_host: "localhost".into(),
        server_port: 25565,
        next_state,
    };
    send_packet(
        stream,
        ServerboundHandshakeSetProtocol::PACKET_ID,
        &handshake,
    )
    .await;
}

// -- Status e2e tests --

#[tokio::test]
async fn e2e_status_ping_full_flow() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Server task
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let conn = Connection::<Handshake>::accept(stream);

        let HandshakeResult::Status(mut conn, handshake) = conn.read_handshake().await.unwrap()
        else {
            panic!("expected Status");
        };
        assert_eq!(handshake.protocol_version, 767);

        // Read StatusRequest
        let packet = conn.read_packet().await.unwrap();
        assert!(matches!(packet, ServerboundStatusPacket::PingStart(_)));

        // Send StatusResponse
        let response = ClientboundStatusServerInfo {
            response:
                r#"{"version":{"name":"1.21","protocol":767},"description":{"text":"E2E Test"}}"#
                    .into(),
        };
        conn.write_status_response(&response).await.unwrap();

        // Read Ping, send Pong
        let packet = conn.read_packet().await.unwrap();
        if let ServerboundStatusPacket::Ping(ping) = packet {
            let pong = ClientboundStatusPing { time: ping.time };
            conn.write_ping_response(&pong).await.unwrap();
        } else {
            panic!("expected Ping");
        }
    });

    // Client
    let mut client = TcpStream::connect(addr).await.unwrap();

    // Send Handshake (next_state = 1 = Status)
    client_handshake(&mut client, 1).await;

    // Send StatusRequest
    send_packet(
        &mut client,
        ServerboundStatusPingStart::PACKET_ID,
        &ServerboundStatusPingStart,
    )
    .await;

    // Read StatusResponse
    let (id, response): (_, ClientboundStatusServerInfo) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundStatusServerInfo::PACKET_ID);
    assert!(response.response.contains("E2E Test"));

    // Send Ping
    let ping = ServerboundStatusPing { time: 42 };
    send_packet(&mut client, ServerboundStatusPing::PACKET_ID, &ping).await;

    // Read Pong
    let (id, pong): (_, ClientboundStatusPing) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundStatusPing::PACKET_ID);
    assert_eq!(pong.time, 42);

    server.await.unwrap();
}

#[tokio::test]
async fn e2e_status_server_info_content() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let conn = Connection::<Handshake>::accept(stream);
        let HandshakeResult::Status(mut conn, _) = conn.read_handshake().await.unwrap() else {
            panic!("expected Status");
        };

        conn.read_packet().await.unwrap();

        let response = ClientboundStatusServerInfo {
            response: r#"{"version":{"name":"Basalt 1.21","protocol":767},"players":{"max":100,"online":5},"description":{"text":"Hello"}}"#.into(),
        };
        conn.write_status_response(&response).await.unwrap();
    });

    let mut client = TcpStream::connect(addr).await.unwrap();
    client_handshake(&mut client, 1).await;
    send_packet(
        &mut client,
        ServerboundStatusPingStart::PACKET_ID,
        &ServerboundStatusPingStart,
    )
    .await;

    let (_, response): (_, ClientboundStatusServerInfo) = recv_packet(&mut client).await;
    assert!(response.response.contains("Basalt 1.21"));
    assert!(response.response.contains("\"max\":100"));
    assert!(response.response.contains("\"online\":5"));

    server.await.unwrap();
}

// -- Login e2e tests --

#[tokio::test]
async fn e2e_login_start_then_disconnect() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let conn = Connection::<Handshake>::accept(stream);

        let HandshakeResult::Login(mut conn, handshake) = conn.read_handshake().await.unwrap()
        else {
            panic!("expected Login");
        };
        assert_eq!(handshake.protocol_version, 767);

        // Read LoginStart
        let packet = conn.read_packet().await.unwrap();
        match packet {
            ServerboundLoginPacket::LoginStart(login) => {
                assert_eq!(login.username, "TestPlayer");
            }
            _ => panic!("expected LoginStart"),
        }

        // Send Disconnect
        conn.disconnect(r#"{"text":"Go away"}"#).await.unwrap();
    });

    // Client
    let mut client = TcpStream::connect(addr).await.unwrap();

    // Send Handshake (next_state = 2 = Login)
    client_handshake(&mut client, 2).await;

    // Send LoginStart
    let login = ServerboundLoginLoginStart {
        username: "TestPlayer".into(),
        player_uuid: Uuid::new(0, 0),
    };
    send_packet(&mut client, ServerboundLoginLoginStart::PACKET_ID, &login).await;

    // Read Disconnect
    let (id, disconnect): (_, ClientboundLoginDisconnect) = recv_packet(&mut client).await;
    assert_eq!(id, ClientboundLoginDisconnect::PACKET_ID);
    assert!(disconnect.reason.contains("Go away"));

    server.await.unwrap();
}

#[tokio::test]
async fn e2e_login_with_uuid() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let conn = Connection::<Handshake>::accept(stream);
        let HandshakeResult::Login(mut conn, _) = conn.read_handshake().await.unwrap() else {
            panic!("expected Login");
        };

        let packet = conn.read_packet().await.unwrap();
        match packet {
            ServerboundLoginPacket::LoginStart(login) => {
                assert_eq!(login.username, "Notch");
                assert_eq!(
                    login.player_uuid,
                    Uuid::new(0x069a79f444e94726, 0xa5befca90e38aaf5)
                );
            }
            _ => panic!("expected LoginStart"),
        }

        conn.disconnect(r#"{"text":"Verified"}"#).await.unwrap();
    });

    let mut client = TcpStream::connect(addr).await.unwrap();
    client_handshake(&mut client, 2).await;

    let login = ServerboundLoginLoginStart {
        username: "Notch".into(),
        player_uuid: Uuid::new(0x069a79f444e94726, 0xa5befca90e38aaf5),
    };
    send_packet(&mut client, ServerboundLoginLoginStart::PACKET_ID, &login).await;

    let (_, disconnect): (_, ClientboundLoginDisconnect) = recv_packet(&mut client).await;
    assert!(disconnect.reason.contains("Verified"));

    server.await.unwrap();
}

// -- Error cases --

#[tokio::test]
async fn e2e_client_disconnect_before_handshake() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let conn = Connection::<Handshake>::accept(stream);
        // Client will disconnect immediately — server should get an error
        assert!(conn.read_handshake().await.is_err());
    });

    let client = TcpStream::connect(addr).await.unwrap();
    drop(client); // Disconnect immediately

    server.await.unwrap();
}

#[tokio::test]
async fn e2e_multiple_connections_sequential() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        // Handle 3 connections sequentially
        for i in 0..3 {
            let (stream, _) = listener.accept().await.unwrap();
            let conn = Connection::<Handshake>::accept(stream);
            let HandshakeResult::Status(mut conn, _) = conn.read_handshake().await.unwrap() else {
                panic!("expected Status");
            };

            conn.read_packet().await.unwrap();

            let response = ClientboundStatusServerInfo {
                response: format!(r#"{{"description":{{"text":"Connection {i}"}}}}"#),
            };
            conn.write_status_response(&response).await.unwrap();
        }
    });

    for i in 0..3 {
        let mut client = TcpStream::connect(addr).await.unwrap();
        client_handshake(&mut client, 1).await;
        send_packet(
            &mut client,
            ServerboundStatusPingStart::PACKET_ID,
            &ServerboundStatusPingStart,
        )
        .await;

        let (_, response): (_, ClientboundStatusServerInfo) = recv_packet(&mut client).await;
        assert!(response.response.contains(&format!("Connection {i}")));
    }

    server.await.unwrap();
}
