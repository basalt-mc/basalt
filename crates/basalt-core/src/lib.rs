//! Basalt core abstractions and shared types.
//!
//! This crate provides the foundational types and traits shared
//! across the Basalt server ecosystem:
//!
//! - [`Context`] trait — abstraction over execution environments
//! - [`components`] — ECS component types and marker trait
//! - [`Gamemode`] — type-safe gamemode enum
//! - [`BroadcastMessage`] — cross-player message types
//! - [`PlayerSnapshot`] — player state snapshots
//! - [`ProfileProperty`] — Mojang profile data
//! - [`PluginLogger`] — scoped logging for plugins

pub mod broadcast;
pub mod components;
pub mod context;
pub mod gamemode;
pub mod logger;
pub mod system;
pub mod testing;

pub use broadcast::{BroadcastMessage, PlayerSnapshot, ProfileProperty};
pub use components::{
    BlockPosition, BoundingBox, ChunkPosition, Component, DroppedItem, EntityId, EntityKind,
    Health, Inventory, Lifetime, OpenContainer, Phase, PickupDelay, PlayerRef, Position, Rotation,
    Sneaking, Velocity,
};
pub use context::{
    ChatContext, ContainerContext, Context, EntityContext, PlayerContext, WorldContext,
};
pub use gamemode::Gamemode;
pub use logger::PluginLogger;
pub use system::{
    SystemAccess, SystemBuilder, SystemContext, SystemContextExt, SystemDescriptor, SystemRunner,
};
