//! Minecraft packet definitions organized by connection state.
//!
//! Each submodule contains the packet structs for one connection state,
//! along with direction-specific enums (serverbound/clientbound) that
//! enable exhaustive pattern matching on received packets.

pub mod handshake;
pub mod status;
