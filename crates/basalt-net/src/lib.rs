//! Async networking layer for the Minecraft protocol.
//!
//! Handles TCP connection management with VarInt length-prefixed framing
//! and type-safe connection state transitions. The connection typestate
//! pattern uses Rust's type system to enforce the Minecraft protocol
//! state machine at compile time: Handshake → Status/Login → etc.
//!
//! Encryption (AES/CFB-8), compression (zlib), and the middleware
//! pipeline will be added in subsequent issues.

pub mod compression;
pub mod connection;
pub mod crypto;
pub mod error;
pub mod framing;

pub use connection::Connection;
pub use error::{Error, Result};
