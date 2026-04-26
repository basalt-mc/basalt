//! Server-specific error types.
//!
//! Wraps errors from the networking layer, protocol, and HTTP (skin
//! fetching) into a single `Error` enum with a `Result` alias.

/// Server error type encompassing all failure modes.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An error from the networking layer (TCP, framing, encryption).
    #[error("network error: {0}")]
    Net(#[from] basalt_net::Error),

    /// An error from the protocol layer (packet decoding, unknown IDs).
    #[error("protocol error: {0}")]
    Protocol(#[from] basalt_mc_protocol::Error),

    /// An error from an HTTP request (skin fetching, Mojang API).
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
}

/// Result alias using the server `Error` type.
pub type Result<T> = std::result::Result<T, Error>;
