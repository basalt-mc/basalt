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
/// Currently only supports Handshake and Status states. Login, Configuration,
/// and Play will be added as their packets are implemented.
///
/// # Usage
///
/// ```ignore
/// let registry = PacketRegistry::for_version(ProtocolVersion::V1_21);
/// let packet = registry.decode_serverbound_status(0x00, &mut buf)?;
/// match packet {
///     ServerboundStatusPacket::StatusRequest(_) => { /* handle */ },
///     ServerboundStatusPacket::PingRequest(ping) => { /* handle */ },
/// }
/// ```
#[derive(Debug)]
pub struct PacketRegistry {
    /// The protocol version this registry was built for.
    version: ProtocolVersion,
}

impl PacketRegistry {
    /// Creates a packet registry for the given protocol version.
    ///
    /// The registry knows which packet IDs map to which structs for
    /// this version. Currently all supported versions share the same
    /// mappings for Handshake and Status packets.
    pub fn for_version(version: ProtocolVersion) -> Self {
        Self { version }
    }

    /// Returns the protocol version this registry was built for.
    pub fn version(&self) -> ProtocolVersion {
        self.version
    }

    /// Decodes a serverbound Handshake packet from its ID and payload.
    ///
    /// The packet ID (VarInt) should already be read from the stream.
    /// The buffer should contain the remaining payload bytes.
    pub fn decode_serverbound_handshake(
        &self,
        id: i32,
        buf: &mut &[u8],
    ) -> Result<ServerboundHandshakePacket> {
        ServerboundHandshakePacket::decode_by_id(id, buf)
    }

    /// Decodes a serverbound Status packet from its ID and payload.
    ///
    /// The packet ID (VarInt) should already be read from the stream.
    /// The buffer should contain the remaining payload bytes.
    pub fn decode_serverbound_status(
        &self,
        id: i32,
        buf: &mut &[u8],
    ) -> Result<ServerboundStatusPacket> {
        ServerboundStatusPacket::decode_by_id(id, buf)
    }

    /// Decodes a clientbound Status packet from its ID and payload.
    ///
    /// The packet ID (VarInt) should already be read from the stream.
    /// The buffer should contain the remaining payload bytes.
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
    use crate::packets::handshake::HandshakePacket;
    use crate::packets::status::{PingRequest, PingResponse, StatusRequest, StatusResponse};
    use basalt_types::{Encode, EncodedSize};

    fn registry() -> PacketRegistry {
        PacketRegistry::for_version(ProtocolVersion::V1_21)
    }

    #[test]
    fn version() {
        assert_eq!(registry().version(), ProtocolVersion::V1_21);
    }

    // -- Handshake --

    #[test]
    fn decode_handshake() {
        let packet = HandshakePacket {
            protocol_version: 767,
            server_address: "localhost".into(),
            server_port: 25565,
            next_state: 1,
        };
        let mut buf = Vec::with_capacity(packet.encoded_size());
        packet.encode(&mut buf).unwrap();

        // Skip packet ID byte
        let mut cursor = buf.as_slice();
        let result = registry()
            .decode_serverbound_handshake(0x00, &mut cursor)
            .unwrap();
        assert!(cursor.is_empty());
        assert_eq!(result, ServerboundHandshakePacket::Handshake(packet));
    }

    // -- Status serverbound --

    #[test]
    fn decode_status_request() {
        let mut buf = Vec::new();
        StatusRequest.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let result = registry()
            .decode_serverbound_status(0x00, &mut cursor)
            .unwrap();
        assert!(matches!(result, ServerboundStatusPacket::StatusRequest(_)));
    }

    #[test]
    fn decode_ping_request() {
        let packet = PingRequest { payload: 12345 };
        let mut buf = Vec::new();
        packet.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let result = registry()
            .decode_serverbound_status(0x01, &mut cursor)
            .unwrap();
        assert_eq!(
            result,
            ServerboundStatusPacket::PingRequest(PingRequest { payload: 12345 })
        );
    }

    // -- Status clientbound --

    #[test]
    fn decode_status_response() {
        let packet = StatusResponse {
            json_response: "{}".into(),
        };
        let mut buf = Vec::new();
        packet.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let result = registry()
            .decode_clientbound_status(0x00, &mut cursor)
            .unwrap();
        assert_eq!(
            result,
            ClientboundStatusPacket::StatusResponse(StatusResponse {
                json_response: "{}".into()
            })
        );
    }

    #[test]
    fn decode_ping_response() {
        let packet = PingResponse { payload: 67890 };
        let mut buf = Vec::new();
        packet.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let result = registry()
            .decode_clientbound_status(0x01, &mut cursor)
            .unwrap();
        assert_eq!(
            result,
            ClientboundStatusPacket::PingResponse(PingResponse { payload: 67890 })
        );
    }

    // -- Unknown packets --

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
