use crate::error::Result;
use crate::packets::handshake::ServerboundHandshakePacket;
use crate::packets::status::{ClientboundStatusPacket, ServerboundStatusPacket};
use crate::version::ProtocolVersion;

/// Version-aware packet registry for decoding Minecraft protocol packets.
///
/// The registry dispatches raw packet bytes to the correct decoder based on
/// the packet ID, connection state, and direction (serverbound/clientbound).
/// It is constructed for a specific protocol version via `PacketRegistry::for_version`.
///
/// Currently supports Handshake, Status, and Login states. Configuration and
/// Play will be added as their packets are generated.
#[derive(Debug)]
pub struct PacketRegistry {
    /// The protocol version this registry was built for.
    version: ProtocolVersion,
}

impl PacketRegistry {
    /// Creates a packet registry for the given protocol version.
    pub fn for_version(version: ProtocolVersion) -> Self {
        Self { version }
    }

    /// Returns the protocol version this registry was built for.
    pub fn version(&self) -> ProtocolVersion {
        self.version
    }

    /// Decodes a serverbound Handshake packet from its ID and payload.
    pub fn decode_serverbound_handshake(
        &self,
        id: i32,
        buf: &mut &[u8],
    ) -> Result<ServerboundHandshakePacket> {
        ServerboundHandshakePacket::decode_by_id(id, buf)
    }

    /// Decodes a serverbound Status packet from its ID and payload.
    pub fn decode_serverbound_status(
        &self,
        id: i32,
        buf: &mut &[u8],
    ) -> Result<ServerboundStatusPacket> {
        ServerboundStatusPacket::decode_by_id(id, buf)
    }

    /// Decodes a clientbound Status packet from its ID and payload.
    pub fn decode_clientbound_status(
        &self,
        id: i32,
        buf: &mut &[u8],
    ) -> Result<ClientboundStatusPacket> {
        ClientboundStatusPacket::decode_by_id(id, buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packets::handshake::ServerboundHandshakeSetProtocol;
    use crate::packets::status::{
        ClientboundStatusPing, ClientboundStatusServerInfo, ServerboundStatusPing,
        ServerboundStatusPingStart,
    };
    use basalt_types::{Encode, EncodedSize};

    fn registry() -> PacketRegistry {
        PacketRegistry::for_version(ProtocolVersion::V1_21)
    }

    #[test]
    fn version() {
        assert_eq!(registry().version(), ProtocolVersion::V1_21);
    }

    #[test]
    fn decode_handshake() {
        let packet = ServerboundHandshakeSetProtocol {
            protocol_version: 767,
            server_host: "localhost".into(),
            server_port: 25565,
            next_state: 1,
        };
        let mut buf = Vec::with_capacity(packet.encoded_size());
        packet.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let result = registry()
            .decode_serverbound_handshake(ServerboundHandshakeSetProtocol::PACKET_ID, &mut cursor)
            .unwrap();
        assert!(cursor.is_empty());
        assert_eq!(result, ServerboundHandshakePacket::SetProtocol(packet));
    }

    #[test]
    fn decode_status_request() {
        let mut buf = Vec::new();
        ServerboundStatusPingStart.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let result = registry()
            .decode_serverbound_status(ServerboundStatusPingStart::PACKET_ID, &mut cursor)
            .unwrap();
        assert!(matches!(result, ServerboundStatusPacket::PingStart(_)));
    }

    #[test]
    fn decode_ping_request() {
        let packet = ServerboundStatusPing { time: 12345 };
        let mut buf = Vec::new();
        packet.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let result = registry()
            .decode_serverbound_status(ServerboundStatusPing::PACKET_ID, &mut cursor)
            .unwrap();
        assert_eq!(
            result,
            ServerboundStatusPacket::Ping(ServerboundStatusPing { time: 12345 })
        );
    }

    #[test]
    fn decode_status_response() {
        let packet = ClientboundStatusServerInfo {
            response: "{}".into(),
        };
        let mut buf = Vec::new();
        packet.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let result = registry()
            .decode_clientbound_status(ClientboundStatusServerInfo::PACKET_ID, &mut cursor)
            .unwrap();
        assert_eq!(
            result,
            ClientboundStatusPacket::ServerInfo(ClientboundStatusServerInfo {
                response: "{}".into()
            })
        );
    }

    #[test]
    fn decode_ping_response() {
        let packet = ClientboundStatusPing { time: 67890 };
        let mut buf = Vec::new();
        packet.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let result = registry()
            .decode_clientbound_status(ClientboundStatusPing::PACKET_ID, &mut cursor)
            .unwrap();
        assert_eq!(
            result,
            ClientboundStatusPacket::Ping(ClientboundStatusPing { time: 67890 })
        );
    }

    #[test]
    fn unknown_handshake_packet() {
        let mut cursor: &[u8] = &[];
        assert!(
            registry()
                .decode_serverbound_handshake(0xFF, &mut cursor)
                .is_err()
        );
    }

    #[test]
    fn unknown_status_serverbound() {
        let mut cursor: &[u8] = &[];
        assert!(
            registry()
                .decode_serverbound_status(0xFF, &mut cursor)
                .is_err()
        );
    }

    #[test]
    fn unknown_status_clientbound() {
        let mut cursor: &[u8] = &[];
        assert!(
            registry()
                .decode_clientbound_status(0xFF, &mut cursor)
                .is_err()
        );
    }
}
