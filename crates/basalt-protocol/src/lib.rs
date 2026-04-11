//! Minecraft packet definitions and version-aware packet registry.
//!
//! This crate defines all Minecraft protocol packets as Rust structs with
//! derive-generated `Encode`/`Decode`/`EncodedSize` implementations. Packets
//! are organized by connection state (Handshake, Status, Login, Configuration,
//! Play) and direction (serverbound/clientbound).
//!
//! The `PacketRegistry` provides version-aware packet dispatching: given a
//! protocol version, connection state, direction, and packet ID, it decodes
//! the raw bytes into the correct typed enum variant.

pub mod error;
pub mod packets;
pub mod registry;
pub mod state;
pub mod version;

pub use error::{Error, Result};
pub use registry::PacketRegistry;
pub use state::ConnectionState;
pub use version::ProtocolVersion;

#[cfg(test)]
mod derive_tests;
