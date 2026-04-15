use std::marker::PhantomData;

use tokio::net::TcpStream;

use basalt_protocol::packets::configuration::{
    ClientboundConfigurationDisconnect, ClientboundConfigurationFinishConfiguration,
    ServerboundConfigurationPacket,
};
use basalt_protocol::packets::handshake::{
    ServerboundHandshakePacket, ServerboundHandshakeSetProtocol,
};
use basalt_protocol::packets::login::{
    ClientboundLoginDisconnect, ClientboundLoginSuccess, ServerboundLoginPacket,
};
use basalt_protocol::packets::play::ServerboundPlayPacket;
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

/// Marker type for the Configuration connection state.
///
/// In this state, the server sends registry data, resource packs,
/// feature flags, and tags before transitioning to Play.
pub struct Configuration;

/// Marker type for the Play connection state.
///
/// Active gameplay state. The majority of packets (movement, block
/// changes, chat, entity updates) are exchanged here.
pub struct Play;

/// The result of reading a Handshake packet.
///
/// The client's Handshake declares which state to transition to:
/// Status (server list ping) or Login (joining the game).
pub enum HandshakeResult {
    /// Client wants server status (next_state = 1).
    Status(Connection<Status>, ServerboundHandshakeSetProtocol),
    /// Client wants to log in (next_state = 2).
    Login(Connection<Login>, ServerboundHandshakeSetProtocol),
}

/// A type-safe Minecraft protocol connection.
///
/// The connection wraps a `ProtocolStream` (TCP with optional encryption
/// and compression) and enforces the protocol state machine at compile
/// time. Each state transition consumes the old connection and returns
/// a new one in the next state.
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
        let raw = self.read_raw().await?;

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
            1 => Ok(HandshakeResult::Status(self.transition(), packet)),
            2 => Ok(HandshakeResult::Login(self.transition(), packet)),
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
        let raw = self.read_raw().await?;
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
}

impl Connection<Login> {
    /// Reads a serverbound Login packet from the client.
    pub async fn read_packet(&mut self) -> Result<ServerboundLoginPacket> {
        let raw = self.read_raw().await?;
        let mut cursor = raw.payload.as_slice();
        Ok(ServerboundLoginPacket::decode_by_id(raw.id, &mut cursor)?)
    }

    /// Enables AES-128 CFB-8 encryption on this connection.
    pub fn enable_encryption(&mut self, shared_secret: &[u8; 16]) {
        self.stream.enable_encryption(shared_secret);
    }

    /// Enables zlib compression on this connection.
    pub fn enable_compression(&mut self, threshold: usize) {
        self.stream.enable_compression(threshold);
    }

    /// Sends LoginSuccess and transitions to Configuration.
    ///
    /// After sending this packet, the server waits for the client's
    /// LoginAcknowledged, then transitions to the Configuration state.
    pub async fn send_login_success(
        mut self,
        success: &ClientboundLoginSuccess,
    ) -> Result<Connection<Configuration>> {
        self.write_packet(ClientboundLoginSuccess::PACKET_ID, success)
            .await?;

        // Read packets until LoginAcknowledged, with a limit to prevent
        // malicious clients from stalling the server indefinitely
        let mut attempts = 0;
        loop {
            let raw = self.read_raw().await?;
            attempts += 1;
            if attempts > 100 {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "client did not send LoginAcknowledged after 100 packets",
                )));
            }
            let mut cursor = raw.payload.as_slice();
            let packet = ServerboundLoginPacket::decode_by_id(raw.id, &mut cursor)?;
            if let ServerboundLoginPacket::LoginAcknowledged(_) = packet {
                return Ok(self.transition());
            }
        }
    }

    /// Writes a Disconnect packet to the client.
    pub async fn disconnect(&mut self, reason: &str) -> Result<()> {
        let packet = ClientboundLoginDisconnect {
            reason: reason.to_string(),
        };
        self.write_packet(ClientboundLoginDisconnect::PACKET_ID, &packet)
            .await
    }
}

impl Connection<Configuration> {
    /// Reads a serverbound Configuration packet from the client.
    pub async fn read_packet(&mut self) -> Result<ServerboundConfigurationPacket> {
        let raw = self.read_raw().await?;
        let mut cursor = raw.payload.as_slice();
        Ok(ServerboundConfigurationPacket::decode_by_id(
            raw.id,
            &mut cursor,
        )?)
    }

    /// Writes a clientbound Configuration packet to the client.
    pub async fn write_packet_typed<P: Encode + EncodedSize>(
        &mut self,
        packet_id: i32,
        packet: &P,
    ) -> Result<()> {
        self.write_packet(packet_id, packet).await
    }

    /// Sends FinishConfiguration and transitions to Play.
    ///
    /// After sending this packet, the server waits for the client's
    /// FinishConfiguration acknowledgement, then transitions to Play.
    pub async fn finish_configuration(mut self) -> Result<Connection<Play>> {
        self.write_packet(
            ClientboundConfigurationFinishConfiguration::PACKET_ID,
            &ClientboundConfigurationFinishConfiguration,
        )
        .await?;

        // Read packets until FinishConfiguration — the client may send
        // settings, plugin channels, known packs, or common packets
        // (which our codegen skips) before acknowledging
        loop {
            let raw = self.read_raw().await?;
            let mut cursor = raw.payload.as_slice();
            match ServerboundConfigurationPacket::decode_by_id(raw.id, &mut cursor) {
                Ok(ServerboundConfigurationPacket::FinishConfiguration(_)) => {
                    return Ok(self.transition());
                }
                Ok(_) => {} // Ignore known non-finish packets
                Err(_) => {
                    // Unknown or common packets (settings, brand, resource packs)
                    // are expected here — the client sends optional config data
                    // that our codegen may not cover
                }
            }
        }
    }

    /// Writes a Disconnect packet to the client.
    ///
    /// The reason is an NBT text component (since 1.20.3+).
    pub async fn disconnect(&mut self, reason: basalt_types::NbtCompound) -> Result<()> {
        let packet = ClientboundConfigurationDisconnect { reason };
        self.write_packet(ClientboundConfigurationDisconnect::PACKET_ID, &packet)
            .await
    }
}

impl Connection<Play> {
    /// Reads a serverbound Play packet from the client.
    pub async fn read_packet(&mut self) -> Result<ServerboundPlayPacket> {
        let raw = self.read_raw().await?;
        let mut cursor = raw.payload.as_slice();
        Ok(ServerboundPlayPacket::decode_by_id(raw.id, &mut cursor)?)
    }

    /// Writes a clientbound Play packet to the client.
    pub async fn write_packet_typed<P: Encode + EncodedSize>(
        &mut self,
        packet_id: i32,
        packet: &P,
    ) -> Result<()> {
        self.write_packet(packet_id, packet).await
    }
}

impl crate::writer::PacketWriter for Connection<Play> {
    async fn write_packet_typed<P: Encode + EncodedSize>(
        &mut self,
        packet_id: i32,
        packet: &P,
    ) -> Result<()> {
        self.write_packet(packet_id, packet).await
    }
}

// -- Shared helpers --

impl<S> Connection<S> {
    /// Reads a raw framed packet from the stream.
    async fn read_raw(&mut self) -> Result<crate::framing::RawPacket> {
        self.stream.read_raw_packet().await?.ok_or_else(|| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "connection closed",
            ))
        })
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

    /// Transitions this connection to a new state, consuming the old one.
    fn transition<T>(self) -> Connection<T> {
        Connection {
            stream: self.stream,
            _state: PhantomData,
        }
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

    #[tokio::test]
    async fn login_to_configuration_transition() {
        use basalt_protocol::packets::login::{
            ServerboundLoginLoginAcknowledged, ServerboundLoginLoginStart,
        };
        use basalt_types::Uuid;

        let (server_stream, mut client_stream) = connected_pair().await;

        // Handshake → Login
        client_send(
            &mut client_stream,
            ServerboundHandshakeSetProtocol::PACKET_ID,
            &handshake_packet(2),
        )
        .await;

        let conn = Connection::<Handshake>::accept(server_stream);
        let HandshakeResult::Login(mut conn, _) = conn.read_handshake().await.unwrap() else {
            panic!("expected Login");
        };

        // Client sends LoginStart
        let login_start = ServerboundLoginLoginStart {
            username: "TestPlayer".into(),
            player_uuid: Uuid::new(0, 0),
        };
        client_send(
            &mut client_stream,
            ServerboundLoginLoginStart::PACKET_ID,
            &login_start,
        )
        .await;
        conn.read_packet().await.unwrap();

        // Server sends LoginSuccess → client sends LoginAcknowledged
        let success = ClientboundLoginSuccess::default();
        // Queue LoginAcknowledged from client before calling send_login_success
        client_send(
            &mut client_stream,
            ServerboundLoginLoginAcknowledged::PACKET_ID,
            &ServerboundLoginLoginAcknowledged,
        )
        .await;

        let config_conn = conn.send_login_success(&success).await.unwrap();
        // We're now in Configuration state
        drop(config_conn);
    }
}
