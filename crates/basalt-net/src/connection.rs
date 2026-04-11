use std::marker::PhantomData;

use tokio::net::TcpStream;

use basalt_protocol::packets::handshake::{
    ServerboundHandshakePacket, ServerboundHandshakeSetProtocol,
};
use basalt_protocol::packets::status::{
    ClientboundStatusPing, ClientboundStatusServerInfo, ServerboundStatusPacket,
};
use basalt_types::{Encode, EncodedSize};

use crate::error::{Error, Result};
use crate::framing;

/// Marker type for the Handshake connection state.
///
/// In this state, the server waits for the client's Handshake packet,
/// which declares the protocol version and desired next state (Status
/// or Login). No other packets are valid.
pub struct Handshake;

/// Marker type for the Status connection state.
///
/// In this state, the server exchanges status information with the client:
/// server list data (MOTD, player count, icon) and latency measurement.
/// This is the server list ping flow.
pub struct Status;

/// A type-safe Minecraft protocol connection.
///
/// The connection wraps a TCP stream and enforces the protocol state machine
/// at compile time using Rust's type system. Each state transition consumes
/// the old connection and returns a new one in the next state, making it
/// impossible to call methods for the wrong state.
///
/// The type parameter `S` is a zero-sized marker type that represents the
/// current connection state (Handshake, Status, Login, etc.).
pub struct Connection<S> {
    stream: TcpStream,
    _state: PhantomData<S>,
}

impl Connection<Handshake> {
    /// Wraps a TCP stream as a new Handshake connection.
    ///
    /// This is the entry point for all incoming connections. The client
    /// is expected to send a Handshake packet as its first message.
    pub fn accept(stream: TcpStream) -> Self {
        Self {
            stream,
            _state: PhantomData,
        }
    }

    /// Reads the client's Handshake packet and transitions to the next state.
    ///
    /// Reads a single framed packet from the stream, decodes it as a
    /// Handshake packet, and returns both the packet (for inspection) and
    /// the connection transitioned to the appropriate next state.
    ///
    /// Currently only supports transitioning to Status (next_state = 1).
    /// Login (next_state = 2) will be supported when Login packets are
    /// implemented.
    pub async fn read_handshake(
        mut self,
    ) -> Result<(Connection<Status>, ServerboundHandshakeSetProtocol)> {
        let raw = framing::read_raw_packet(&mut self.stream)
            .await?
            .ok_or_else(|| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "connection closed before handshake",
                ))
            })?;

        let mut cursor = raw.payload.as_slice();
        match ServerboundHandshakePacket::decode_by_id(raw.id, &mut cursor)? {
            ServerboundHandshakePacket::SetProtocol(packet) => Ok((
                Connection {
                    stream: self.stream,
                    _state: PhantomData,
                },
                packet,
            )),
            _ => Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "expected SetProtocol handshake packet",
            ))),
        }
    }
}

impl Connection<Status> {
    /// Reads a serverbound Status packet from the client.
    ///
    /// Returns the decoded packet enum, which is either a StatusRequest
    /// (asking for server info) or a ServerboundStatusPing (latency measurement).
    pub async fn read_packet(&mut self) -> Result<ServerboundStatusPacket> {
        let raw = framing::read_raw_packet(&mut self.stream)
            .await?
            .ok_or_else(|| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "connection closed during status",
                ))
            })?;

        let mut cursor = raw.payload.as_slice();
        Ok(ServerboundStatusPacket::decode_by_id(raw.id, &mut cursor)?)
    }

    /// Writes a ClientboundStatusServerInfo packet to the client.
    ///
    /// Sends the server's status information (MOTD, player count, icon)
    /// as a JSON string. This is the response to a StatusRequest.
    pub async fn write_status_response(
        &mut self,
        response: &ClientboundStatusServerInfo,
    ) -> Result<()> {
        self.write_packet(ClientboundStatusServerInfo::PACKET_ID, response)
            .await
    }

    /// Writes a ClientboundStatusPing packet to the client.
    ///
    /// Echoes back the client's ping time for latency measurement.
    /// This is the response to a ServerboundStatusPing.
    pub async fn write_ping_response(&mut self, response: &ClientboundStatusPing) -> Result<()> {
        self.write_packet(ClientboundStatusPing::PACKET_ID, response)
            .await
    }

    /// Encodes and writes a packet with the given ID to the stream.
    async fn write_packet<P: Encode + EncodedSize>(
        &mut self,
        packet_id: i32,
        packet: &P,
    ) -> Result<()> {
        let mut payload = Vec::with_capacity(packet.encoded_size());
        packet
            .encode(&mut payload)
            .map_err(|e| Error::Protocol(basalt_protocol::Error::Type(e)))?;
        framing::write_raw_packet(&mut self.stream, packet_id, &payload).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use basalt_protocol::packets::status::{ServerboundStatusPing, ServerboundStatusPingStart};
    use basalt_types::Decode as _;
    use tokio::net::TcpListener;

    /// Helper: creates a connected pair of TcpStreams via a local listener.
    async fn connected_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();
        (server, client)
    }

    /// Helper: writes a framed packet from the client side.
    async fn client_send<P: Encode + EncodedSize>(
        stream: &mut TcpStream,
        packet_id: i32,
        packet: &P,
    ) {
        let mut payload = Vec::with_capacity(packet.encoded_size());
        packet.encode(&mut payload).unwrap();
        framing::write_raw_packet(stream, packet_id, &payload)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn handshake_to_status_transition() {
        let (server_stream, mut client_stream) = connected_pair().await;

        // Client sends handshake
        let handshake = ServerboundHandshakeSetProtocol {
            protocol_version: 767,
            server_host: "localhost".into(),
            server_port: 25565,
            next_state: 1,
        };
        client_send(
            &mut client_stream,
            ServerboundHandshakeSetProtocol::PACKET_ID,
            &handshake,
        )
        .await;

        // Server reads handshake and transitions to Status
        let conn = Connection::<Handshake>::accept(server_stream);
        let (mut conn, received_handshake) = conn.read_handshake().await.unwrap();
        assert_eq!(received_handshake, handshake);

        // Client sends StatusRequest
        client_send(
            &mut client_stream,
            ServerboundStatusPingStart::PACKET_ID,
            &ServerboundStatusPingStart,
        )
        .await;

        // Server reads StatusRequest
        let packet = conn.read_packet().await.unwrap();
        assert!(matches!(packet, ServerboundStatusPacket::PingStart(_)));
    }

    #[tokio::test]
    async fn full_status_ping_flow() {
        let (server_stream, mut client_stream) = connected_pair().await;

        // Client sends handshake
        let handshake = ServerboundHandshakeSetProtocol {
            protocol_version: 767,
            server_host: "localhost".into(),
            server_port: 25565,
            next_state: 1,
        };
        client_send(
            &mut client_stream,
            ServerboundHandshakeSetProtocol::PACKET_ID,
            &handshake,
        )
        .await;

        // Server accepts and transitions
        let conn = Connection::<Handshake>::accept(server_stream);
        let (mut conn, _) = conn.read_handshake().await.unwrap();

        // Client sends StatusRequest
        client_send(
            &mut client_stream,
            ServerboundStatusPingStart::PACKET_ID,
            &ServerboundStatusPingStart,
        )
        .await;
        let packet = conn.read_packet().await.unwrap();
        assert!(matches!(packet, ServerboundStatusPacket::PingStart(_)));

        // Server sends ClientboundStatusServerInfo
        let response = ClientboundStatusServerInfo {
            response: r#"{"version":{"name":"1.21","protocol":767}}"#.into(),
        };
        conn.write_status_response(&response).await.unwrap();

        // Client reads ClientboundStatusServerInfo
        let raw = framing::read_raw_packet(&mut client_stream)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(raw.id, ClientboundStatusServerInfo::PACKET_ID);

        // Client sends ServerboundStatusPing
        let ping = ServerboundStatusPing { time: 1234567890 };
        client_send(&mut client_stream, ServerboundStatusPing::PACKET_ID, &ping).await;

        // Server reads ServerboundStatusPing and responds
        let packet = conn.read_packet().await.unwrap();
        match packet {
            ServerboundStatusPacket::Ping(req) => {
                assert_eq!(req.time, 1234567890);
                let pong = ClientboundStatusPing { time: req.time };
                conn.write_ping_response(&pong).await.unwrap();
            }
            _ => panic!("expected ServerboundStatusPing"),
        }

        // Client reads ClientboundStatusPing
        let raw = framing::read_raw_packet(&mut client_stream)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(raw.id, ClientboundStatusPing::PACKET_ID);
        let mut cursor = raw.payload.as_slice();
        let pong = ClientboundStatusPing::decode(&mut cursor).unwrap();
        assert_eq!(pong.time, 1234567890);
    }

    #[tokio::test]
    async fn handshake_eof_returns_error() {
        let (server_stream, client_stream) = connected_pair().await;
        drop(client_stream); // Close immediately

        let conn = Connection::<Handshake>::accept(server_stream);
        assert!(conn.read_handshake().await.is_err());
    }
}
