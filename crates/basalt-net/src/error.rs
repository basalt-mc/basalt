/// Crate-level result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during network operations.
///
/// Covers IO failures (TCP read/write), protocol-level errors (unknown
/// packets, malformed data), and framing errors (oversized packets).
/// Higher layers should map these errors to appropriate disconnect
/// reasons for the client.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A TCP read or write operation failed.
    ///
    /// This includes connection resets, broken pipes, timeouts, and
    /// other OS-level socket errors. Usually means the client disconnected.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// A protocol-level error occurred during packet encoding or decoding.
    ///
    /// Wraps errors from basalt-protocol: unknown packet IDs, type
    /// serialization failures (buffer underflow, invalid data, etc.).
    #[error(transparent)]
    Protocol(#[from] basalt_mc_protocol::Error),

    /// A packet exceeded the maximum allowed size.
    ///
    /// The Minecraft protocol limits packet size to prevent memory
    /// exhaustion from malicious or corrupted data. The default limit
    /// is 2 MiB (2,097,152 bytes). Connections sending oversized packets
    /// should be disconnected.
    #[error("packet too large: {size} bytes, max {max}")]
    PacketTooLarge {
        /// The actual packet size in bytes.
        size: usize,
        /// The maximum allowed packet size in bytes.
        max: usize,
    },
}
