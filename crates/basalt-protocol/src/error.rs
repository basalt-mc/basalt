/// Crate-level result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during packet encoding, decoding, or registry dispatch.
///
/// This error type wraps `basalt_types::Error` for lower-level serialization
/// failures and adds protocol-specific errors like unknown packet IDs.
/// Higher layers (basalt-net) wrap this error in turn via `#[from]`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A packet ID was not recognized for the given connection state.
    ///
    /// This occurs when the registry receives a packet ID that doesn't
    /// match any known packet in the current state and direction. May
    /// indicate a protocol version mismatch, a corrupted stream, or
    /// an unimplemented packet.
    #[error("unknown packet ID {id:#04x} in state {state}")]
    UnknownPacket {
        /// The unrecognized packet ID.
        id: i32,
        /// The connection state where the packet was received.
        state: &'static str,
    },

    /// A lower-level type encoding/decoding error.
    ///
    /// Wraps errors from basalt-types: buffer underflow, invalid data,
    /// VarInt overflow, string length violations, UTF-8 errors, and NBT
    /// parsing failures.
    #[error(transparent)]
    Type(#[from] basalt_types::Error),
}
