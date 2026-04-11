use basalt_derive::packet;
use basalt_types::Decode as _;

use crate::error::{Error, Result};

/// The Handshake packet sent by the client as the very first packet.
///
/// This is the only packet in the Handshake state. It declares the
/// protocol version the client is using, the server address it connected
/// to, and whether it wants to proceed to Status (server list ping) or
/// Login (joining the game).
///
/// After sending this packet, the client transitions to the state
/// indicated by `next_state`.
#[derive(Debug, Clone, PartialEq)]
#[packet(id = 0x00)]
pub struct HandshakePacket {
    /// The protocol version number (e.g., 767 for Minecraft 1.21).
    /// Encoded as a VarInt on the wire.
    #[field(varint)]
    pub protocol_version: i32,

    /// The hostname or IP address used to connect. This may differ from
    /// the actual server address due to SRV records or proxy forwarding.
    pub server_address: String,

    /// The port used to connect. Typically 25565 for standard servers.
    pub server_port: u16,

    /// The desired next state: 1 for Status, 2 for Login, 3 for Transfer.
    /// Encoded as a VarInt on the wire.
    #[field(varint)]
    pub next_state: i32,
}

/// Serverbound packets in the Handshake state.
///
/// The Handshake state contains exactly one serverbound packet. This enum
/// exists for consistency with other states that have multiple packets,
/// and to enable uniform dispatch in the packet registry.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerboundHandshakePacket {
    /// The initial handshake packet (0x00).
    Handshake(HandshakePacket),
}

impl ServerboundHandshakePacket {
    /// Decodes a serverbound Handshake packet from its ID and payload.
    ///
    /// The packet ID has already been read from the framing layer. This
    /// function reads the remaining payload bytes for the identified packet.
    ///
    /// Returns `Error::UnknownPacket` if the ID doesn't match any known
    /// serverbound Handshake packet.
    pub fn decode_by_id(id: i32, buf: &mut &[u8]) -> Result<Self> {
        match id {
            HandshakePacket::PACKET_ID => Ok(Self::Handshake(HandshakePacket::decode(buf)?)),
            _ => Err(Error::UnknownPacket {
                id,
                state: "handshake",
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use basalt_types::{Encode as _, EncodedSize as _};

    #[test]
    fn handshake_roundtrip() {
        let original = HandshakePacket {
            protocol_version: 767,
            server_address: "localhost".into(),
            server_port: 25565,
            next_state: 1,
        };
        let mut buf = Vec::with_capacity(original.encoded_size());
        original.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let decoded = HandshakePacket::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, original);
    }

    #[test]
    fn handshake_dispatch() {
        let packet = HandshakePacket {
            protocol_version: 767,
            server_address: "mc.example.com".into(),
            server_port: 25565,
            next_state: 2,
        };
        let mut buf = Vec::new();
        packet.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let dispatched = ServerboundHandshakePacket::decode_by_id(0x00, &mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(dispatched, ServerboundHandshakePacket::Handshake(packet));
    }

    #[test]
    fn handshake_unknown_id() {
        let mut cursor: &[u8] = &[];
        assert!(matches!(
            ServerboundHandshakePacket::decode_by_id(0x01, &mut cursor),
            Err(Error::UnknownPacket { id: 0x01, .. })
        ));
    }
}
