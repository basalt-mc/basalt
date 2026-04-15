//! Basalt core abstractions and shared types.
//!
//! This crate provides the foundational types and traits shared
//! across the Basalt server ecosystem:
//!
//! - [`Context`] trait — abstraction over execution environments
//! - [`Gamemode`] — type-safe gamemode enum
//! - [`BroadcastMessage`] — cross-player message types
//! - [`PlayerSnapshot`] — player state snapshots
//! - [`ProfileProperty`] — Mojang profile data
//! - [`PluginLogger`] — scoped logging for plugins

pub mod broadcast;
pub mod context;
pub mod gamemode;
pub mod logger;

pub use broadcast::{BroadcastMessage, PlayerSnapshot, ProfileProperty};
pub use context::Context;
pub use gamemode::Gamemode;
pub use logger::PluginLogger;
