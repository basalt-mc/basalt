/// The connection states in the Minecraft protocol.
///
/// A Minecraft connection progresses through these states in order:
/// Handshake → Status or Login → Configuration → Play. Each state
/// has its own set of valid packets (both serverbound and clientbound),
/// and the packet ID space is independent per state — the same ID
/// can mean different packets in different states.
///
/// The Handshake state is special: it contains only one serverbound
/// packet that determines whether the connection transitions to
/// Status (server list ping) or Login (joining the game).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionState {
    /// Initial state. The client sends a single Handshake packet that
    /// declares the protocol version and whether it wants Status or Login.
    Handshake,

    /// Server list ping. The client requests server info (MOTD, player
    /// count, icon) and latency measurement. No authentication occurs.
    Status,

    /// Authentication and encryption setup. The client and server exchange
    /// login credentials, enable encryption (AES/CFB-8), and optionally
    /// enable compression. Ends with Login Success.
    Login,

    /// Post-login configuration. Registry data, resource packs, and
    /// feature flags are exchanged before entering gameplay. Added in
    /// Minecraft 1.20.2.
    Configuration,

    /// Active gameplay. The majority of packets (movement, block changes,
    /// chat, entity updates, chunk data) are exchanged in this state.
    Play,
}

impl ConnectionState {
    /// Returns the protocol name of this state, used in error messages
    /// and debug output.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Handshake => "handshake",
            Self::Status => "status",
            Self::Login => "login",
            Self::Configuration => "configuration",
            Self::Play => "play",
        }
    }
}

impl std::fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_all_states() {
        assert_eq!(ConnectionState::Handshake.as_str(), "handshake");
        assert_eq!(ConnectionState::Status.as_str(), "status");
        assert_eq!(ConnectionState::Login.as_str(), "login");
        assert_eq!(ConnectionState::Configuration.as_str(), "configuration");
        assert_eq!(ConnectionState::Play.as_str(), "play");
    }

    #[test]
    fn display_all_states() {
        assert_eq!(ConnectionState::Handshake.to_string(), "handshake");
        assert_eq!(ConnectionState::Status.to_string(), "status");
        assert_eq!(ConnectionState::Login.to_string(), "login");
        assert_eq!(ConnectionState::Configuration.to_string(), "configuration");
        assert_eq!(ConnectionState::Play.to_string(), "play");
    }
}
