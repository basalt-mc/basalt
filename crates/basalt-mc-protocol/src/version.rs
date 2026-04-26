/// Supported Minecraft protocol versions.
///
/// Each variant corresponds to a specific Minecraft release and its
/// associated protocol version number. The protocol version is sent
/// in the Handshake packet and determines which packet definitions
/// and ID mappings to use.
///
/// Currently only 1.21.x is supported. Additional versions will be
/// added as the multi-version delta/overlay model is implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProtocolVersion {
    /// Minecraft 1.21 (protocol version 767).
    V1_21,
}

impl ProtocolVersion {
    /// Returns the numeric protocol version sent in the Handshake packet.
    ///
    /// This integer identifies the protocol version to the server and is
    /// used to select the correct packet registry. The client sends this
    /// value during the handshake; the server uses it to determine which
    /// packet definitions apply.
    pub fn protocol_number(&self) -> i32 {
        match self {
            Self::V1_21 => 767,
        }
    }

    /// Attempts to resolve a protocol version from its numeric identifier.
    ///
    /// Returns `None` if the protocol number doesn't match any supported
    /// version. This is used during handshake to determine if the server
    /// supports the client's protocol version.
    pub fn from_protocol_number(number: i32) -> Option<Self> {
        match number {
            767 => Some(Self::V1_21),
            _ => None,
        }
    }
}

impl std::fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::V1_21 => f.write_str("1.21"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_number_v1_21() {
        assert_eq!(ProtocolVersion::V1_21.protocol_number(), 767);
    }

    #[test]
    fn from_protocol_number_valid() {
        assert_eq!(
            ProtocolVersion::from_protocol_number(767),
            Some(ProtocolVersion::V1_21)
        );
    }

    #[test]
    fn from_protocol_number_unknown() {
        assert_eq!(ProtocolVersion::from_protocol_number(999), None);
    }

    #[test]
    fn display() {
        assert_eq!(ProtocolVersion::V1_21.to_string(), "1.21");
    }
}
