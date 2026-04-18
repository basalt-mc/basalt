//! Generic Entity Component System — pure storage engine.
//!
//! Intentionally simple: `HashMap` per component type, not archetype-based.
//! Sufficient for thousands of entities without the complexity of a full
//! ECS framework like bevy_ecs or specs.
//!
//! - **Entity**: a unique [`EntityId`] (u32). Just an ID, no data.
//! - **Component**: a typed struct stored in a `HashMap<EntityId, T>`.
//! - **System**: a function that runs each tick with access to the ECS.
//!
//! This crate has **zero domain knowledge** — no Minecraft types, no
//! game-specific components. Component types are defined in `basalt-core`.

mod ecs;
mod system;

pub use ecs::{Component, Ecs, EntityId};
pub use system::{Phase, SystemAccess, SystemBuilder, SystemDescriptor, SystemRunner};
