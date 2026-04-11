use std::marker::PhantomData;

use tokio::net::TcpStream;

use basalt_protocol::packets::handshake::{
    ServerboundHandshakePacket, ServerboundHandshakeSetProtocol,
};
use basalt_protocol::packets::login::{ClientboundLoginDisconnect, ServerboundLoginPacket};
use basalt_protocol::packets::status::{
    ClientboundStatusPing, ClientboundStatusServerInfo, ServerboundStatusPacket,
};
use basalt_types::{Encode, EncodedSize};

use crate::error::{Error, Result};
use crate::stream::ProtocolStream;

/// Marker type for the Handshake connection state.
pub struct Handshake;

/// Marker type for the Status connection state.
pub struct Status;

/// Marker type for the Login connection state.
pub struct Login;

/// The result of reading a Handshake packet.
///
/// The client's Handshake declares which state to transition to:
/// Status (server list ping) or Login (joining the game). This enum
/// lets the caller handle both cases with exhaustive pattern matching.
pub enum HandshakeResult {
    /// Client wants server status (next_state = 1).
    Status(Connection<Status>, ServerboundHandshakeSetProtocol),
    /// Client wants to log in (next_state = 2).
    Login(Connection<Login>, ServerboundHandshakeSetProtocol),
}

/// A type-safe Minecraft protocol connection.
///
/// The connection wraps an `ProtocolStream` (TCP with optional AES/CFB-8
/// encryption) and enforces the protocol state machine at compile time
/// using Rust's type system. Each state transition consumes the old
/// connection and returns a new one in the next state.
pub struct Connection<S> {
    stream: ProtocolStream,
    _state: PhantomData<S>,
}

impl Connection<Handshake> {
    /// Wraps a TCP stream as a new Handshake connection.
    pub fn accept(stream: TcpStream) -> Self {
        Self {
            stream: ProtocolStream::new(stream),
            _state: PhantomData,
        }
    }

    /// Reads the client's Handshake packet and transitions to the next state.
    pub async fn read_handshake(mut self) -> Result<HandshakeResult> {
        let raw = self.stream.read_raw_packet().await?.ok_or_else(|| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "connection closed before handshake",
            ))
        })?;

        let mut cursor = raw.payload.as_slice();
        let packet = match ServerboundHandshakePacket::decode_by_id(raw.id, &mut cursor)? {
            ServerboundHandshakePacket::SetProtocol(p) => p,
            _ => {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "expected SetProtocol handshake packet",
                )));
            }
        };

        match packet.next_state {
            1 => Ok(HandshakeResult::Status(
                Connection {
                    stream: self.stream,
                    _state: PhantomData,
                },
                packet,
            )),
            2 => Ok(HandshakeResult::Login(
                Connection {
                    stream: self.stream,
                    _state: PhantomData,
                },
                packet,
            )),
            other => Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown next_state: {other}"),
            ))),
        }
    }
}

impl Connection<Status> {
    /// Reads a serverbound Status packet from the client.
    pub async fn read_packet(&mut self) -> Result<ServerboundStatusPacket> {
        let raw = self.stream.read_raw_packet().await?.ok_or_else(|| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "connection closed during status",
            ))
        })?;

        let mut cursor = raw.payload.as_slice();
        Ok(ServerboundStatusPacket::decode_by_id(raw.id, &mut cursor)?)
    }

    /// Writes a StatusResponse (server info) packet to the client.
    pub async fn write_status_response(
        &mut self,
        response: &ClientboundStatusServerInfo,
    ) -> Result<()> {
        self.write_packet(ClientboundStatusServerInfo::PACKET_ID, response)
            .await
    }

    /// Writes a PingResponse packet to the client.
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
        self.stream.write_raw_packet(packet_id, &payload).await
    }
}

impl Connection<Login> {
    /// Reads a serverbound Login packet from the client.
    pub async fn read_packet(&mut self) -> Result<ServerboundLoginPacket> {
        let raw = self.stream.read_raw_packet().await?.ok_or_else(|| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "connection closed during login",
            ))
        })?;

        let mut cursor = raw.payload.as_slice();
        Ok(ServerboundLoginPacket::decode_by_id(raw.id, &mut cursor)?)
    }

    /// Enables AES-128 CFB-8 encryption on this connection.
    ///
    /// All subsequent reads and writes will be encrypted/decrypted
    /// transparently. Called after receiving the client's Encryption
    /// Response packet.
    pub fn enable_encryption(&mut self, shared_secret: &[u8; 16]) {
        self.stream.enable_encryption(shared_secret);
    }

    /// Enables zlib compression on this connection.
    ///
    /// Packets with uncompressed size >= threshold bytes will be
    /// zlib-compressed. Called after sending the Set Compression
    /// packet during login.
    pub fn enable_compression(&mut self, threshold: usize) {
        self.stream.enable_compression(threshold);
    }

    /// Writes a Disconnect packet to the client.
    pub async fn disconnect(&mut self, reason: &str) -> Result<()> {
        let packet = ClientboundLoginDisconnect {
            reason: reason.to_string(),
        };
        self.write_packet(ClientboundLoginDisconnect::PACKET_ID, &packet)
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
        self.stream.write_raw_packet(packet_id, &payload).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing;
    use basalt_protocol::packets::status::{ServerboundStatusPing, ServerboundStatusPingStart};
    use basalt_types::Decode as _;
    use tokio::net::TcpListener;

    async fn connected_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();
        (server, client)
    }

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

    fn handshake_packet(next_state: i32) -> ServerboundHandshakeSetProtocol {
        ServerboundHandshakeSetProtocol {
            protocol_version: 767,
            server_host: "localhost".into(),
            server_port: 25565,
            next_state,
        }
    }

    #[tokio::test]
    async fn handshake_to_status() {
        let (server_stream, mut client_stream) = connected_pair().await;

        let handshake = handshake_packet(1);
        client_send(
            &mut client_stream,
            ServerboundHandshakeSetProtocol::PACKET_ID,
            &handshake,
        )
        .await;

        let conn = Connection::<Handshake>::accept(server_stream);
        match conn.read_handshake().await.unwrap() {
            HandshakeResult::Status(mut conn, pkt) => {
                assert_eq!(pkt, handshake);
                client_send(
                    &mut client_stream,
                    ServerboundStatusPingStart::PACKET_ID,
                    &ServerboundStatusPingStart,
                )
                .await;
                let packet = conn.read_packet().await.unwrap();
                assert!(matches!(packet, ServerboundStatusPacket::PingStart(_)));
            }
            _ => panic!("expected Status"),
        }
    }

    #[tokio::test]
    async fn handshake_to_login() {
        let (server_stream, mut client_stream) = connected_pair().await;

        let handshake = handshake_packet(2);
        client_send(
            &mut client_stream,
            ServerboundHandshakeSetProtocol::PACKET_ID,
            &handshake,
        )
        .await;

        let conn = Connection::<Handshake>::accept(server_stream);
        match conn.read_handshake().await.unwrap() {
            HandshakeResult::Login(mut conn, pkt) => {
                assert_eq!(pkt, handshake);
                conn.disconnect(r#"{"text":"Not implemented"}"#)
                    .await
                    .unwrap();
            }
            _ => panic!("expected Login"),
        }
    }

    #[tokio::test]
    async fn full_status_ping_flow() {
        let (server_stream, mut client_stream) = connected_pair().await;

        let handshake = handshake_packet(1);
        client_send(
            &mut client_stream,
            ServerboundHandshakeSetProtocol::PACKET_ID,
            &handshake,
        )
        .await;

        let conn = Connection::<Handshake>::accept(server_stream);
        let HandshakeResult::Status(mut conn, _) = conn.read_handshake().await.unwrap() else {
            panic!("expected Status");
        };

        client_send(
            &mut client_stream,
            ServerboundStatusPingStart::PACKET_ID,
            &ServerboundStatusPingStart,
        )
        .await;
        conn.read_packet().await.unwrap();

        let response = ClientboundStatusServerInfo {
            response: r#"{"version":{"name":"1.21","protocol":767}}"#.into(),
        };
        conn.write_status_response(&response).await.unwrap();
        let raw = framing::read_raw_packet(&mut client_stream)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(raw.id, ClientboundStatusServerInfo::PACKET_ID);

        let ping = ServerboundStatusPing { time: 1234567890 };
        client_send(&mut client_stream, ServerboundStatusPing::PACKET_ID, &ping).await;

        let packet = conn.read_packet().await.unwrap();
        let ServerboundStatusPacket::Ping(req) = packet else {
            panic!("expected Ping");
        };
        let pong = ClientboundStatusPing { time: req.time };
        conn.write_ping_response(&pong).await.unwrap();

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
        drop(client_stream);

        let conn = Connection::<Handshake>::accept(server_stream);
        assert!(conn.read_handshake().await.is_err());
    }
}
