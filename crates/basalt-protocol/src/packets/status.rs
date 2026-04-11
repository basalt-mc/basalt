use basalt_derive::packet;
use basalt_types::Decode as _;

use crate::error::{Error, Result};

// -- Serverbound packets --

/// Status Request packet (serverbound, 0x00).
///
/// Sent by the client to request server status information (MOTD, player
/// count, server icon, protocol version). This packet has no fields —
/// its presence alone triggers the server to respond with a StatusResponse.
#[derive(Debug, Clone, PartialEq)]
#[packet(id = 0x00)]
pub struct StatusRequest;

/// Ping Request packet (serverbound, 0x01).
///
/// Sent by the client to measure round-trip latency. Contains a single
/// i64 payload that the server echoes back in a PingResponse. The client
/// uses the time difference to calculate the server's latency displayed
/// in the server list.
#[derive(Debug, Clone, PartialEq)]
#[packet(id = 0x01)]
pub struct PingRequest {
    /// An arbitrary i64 value chosen by the client, typically the current
    /// timestamp in milliseconds. The server must echo this exact value.
    pub payload: i64,
}

// -- Clientbound packets --

/// Status Response packet (clientbound, 0x00).
///
/// Sent by the server in response to a StatusRequest. Contains a JSON
/// string describing the server's status: version, player count, MOTD,
/// favicon, and other metadata. The JSON format is defined by the
/// Minecraft protocol specification.
#[derive(Debug, Clone, PartialEq)]
#[packet(id = 0x00)]
pub struct StatusResponse {
    /// The server status as a JSON string. Contains fields like
    /// `version`, `players`, `description`, and optionally `favicon`.
    pub json_response: String,
}

/// Ping Response packet (clientbound, 0x01).
///
/// Sent by the server to echo back the client's PingRequest payload.
/// The client compares the echoed value with its original timestamp
/// to calculate round-trip latency.
#[derive(Debug, Clone, PartialEq)]
#[packet(id = 0x01)]
pub struct PingResponse {
    /// The exact i64 value from the client's PingRequest, echoed back
    /// unchanged.
    pub payload: i64,
}

// -- Direction enums --

/// Serverbound packets in the Status state.
///
/// The Status state has two serverbound packets: StatusRequest (empty,
/// triggers server info) and PingRequest (latency measurement).
#[derive(Debug, Clone, PartialEq)]
pub enum ServerboundStatusPacket {
    /// Server status request (0x00).
    StatusRequest(StatusRequest),
    /// Latency measurement request (0x01).
    PingRequest(PingRequest),
}

impl ServerboundStatusPacket {
    /// Decodes a serverbound Status packet from its ID and payload.
    ///
    /// The packet ID has already been read from the framing layer.
    /// Returns `Error::UnknownPacket` for unrecognized IDs.
    pub fn decode_by_id(id: i32, buf: &mut &[u8]) -> Result<Self> {
        match id {
            StatusRequest::PACKET_ID => Ok(Self::StatusRequest(StatusRequest::decode(buf)?)),
            PingRequest::PACKET_ID => Ok(Self::PingRequest(PingRequest::decode(buf)?)),
            _ => Err(Error::UnknownPacket {
                id,
                state: "status",
            }),
        }
    }
}

/// Clientbound packets in the Status state.
///
/// The Status state has two clientbound packets: StatusResponse (server
/// info as JSON) and PingResponse (echoed latency payload).
#[derive(Debug, Clone, PartialEq)]
pub enum ClientboundStatusPacket {
    /// Server status response (0x00).
    StatusResponse(StatusResponse),
    /// Latency measurement response (0x01).
    PingResponse(PingResponse),
}

impl ClientboundStatusPacket {
    /// Decodes a clientbound Status packet from its ID and payload.
    ///
    /// The packet ID has already been read from the framing layer.
    /// Returns `Error::UnknownPacket` for unrecognized IDs.
    pub fn decode_by_id(id: i32, buf: &mut &[u8]) -> Result<Self> {
        match id {
            StatusResponse::PACKET_ID => Ok(Self::StatusResponse(StatusResponse::decode(buf)?)),
            PingResponse::PACKET_ID => Ok(Self::PingResponse(PingResponse::decode(buf)?)),
            _ => Err(Error::UnknownPacket {
                id,
                state: "status",
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use basalt_types::{Encode as _, EncodedSize as _};

    // -- StatusRequest --

    #[test]
    fn status_request_roundtrip() {
        let original = StatusRequest;
        let mut buf = Vec::new();
        original.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let decoded = StatusRequest::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, original);
    }

    // -- PingRequest --

    #[test]
    fn ping_request_roundtrip() {
        let original = PingRequest {
            payload: 1234567890,
        };
        let mut buf = Vec::with_capacity(original.encoded_size());
        original.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let decoded = PingRequest::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, original);
    }

    // -- StatusResponse --

    #[test]
    fn status_response_roundtrip() {
        let original = StatusResponse {
            json_response: r#"{"version":{"name":"1.21","protocol":767},"players":{"max":20,"online":0},"description":{"text":"A Basalt Server"}}"#.into(),
        };
        let mut buf = Vec::with_capacity(original.encoded_size());
        original.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let decoded = StatusResponse::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, original);
    }

    // -- PingResponse --

    #[test]
    fn ping_response_roundtrip() {
        let original = PingResponse {
            payload: 1234567890,
        };
        let mut buf = Vec::with_capacity(original.encoded_size());
        original.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let decoded = PingResponse::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, original);
    }

    // -- Serverbound dispatch --

    #[test]
    fn serverbound_status_request_dispatch() {
        let mut buf = Vec::new();
        StatusRequest.encode(&mut buf).unwrap();

        // Skip packet ID (VarInt 0x00 = 1 byte)
        let mut cursor = buf.as_slice();
        let dispatched = ServerboundStatusPacket::decode_by_id(0x00, &mut cursor).unwrap();
        assert!(matches!(
            dispatched,
            ServerboundStatusPacket::StatusRequest(_)
        ));
    }

    #[test]
    fn serverbound_ping_request_dispatch() {
        let packet = PingRequest { payload: 42 };
        let mut buf = Vec::new();
        packet.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let dispatched = ServerboundStatusPacket::decode_by_id(0x01, &mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(
            dispatched,
            ServerboundStatusPacket::PingRequest(PingRequest { payload: 42 })
        );
    }

    #[test]
    fn serverbound_unknown_id() {
        let mut cursor: &[u8] = &[];
        assert!(matches!(
            ServerboundStatusPacket::decode_by_id(0xFF, &mut cursor),
            Err(Error::UnknownPacket { id: 0xFF, .. })
        ));
    }

    // -- Clientbound dispatch --

    #[test]
    fn clientbound_status_response_dispatch() {
        let packet = StatusResponse {
            json_response: "{}".into(),
        };
        let mut buf = Vec::new();
        packet.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let dispatched = ClientboundStatusPacket::decode_by_id(0x00, &mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(
            dispatched,
            ClientboundStatusPacket::StatusResponse(StatusResponse {
                json_response: "{}".into()
            })
        );
    }

    #[test]
    fn clientbound_ping_response_dispatch() {
        let packet = PingResponse { payload: 99 };
        let mut buf = Vec::new();
        packet.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let dispatched = ClientboundStatusPacket::decode_by_id(0x01, &mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(
            dispatched,
            ClientboundStatusPacket::PingResponse(PingResponse { payload: 99 })
        );
    }

    #[test]
    fn clientbound_unknown_id() {
        let mut cursor: &[u8] = &[];
        assert!(matches!(
            ClientboundStatusPacket::decode_by_id(0xFF, &mut cursor),
            Err(Error::UnknownPacket { id: 0xFF, .. })
        ));
    }
}
