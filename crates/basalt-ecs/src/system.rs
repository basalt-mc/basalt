//! Re-exports system types from `basalt-core` and integrates them
//! with the ECS scheduler.
//!
//! All system types (Phase, SystemDescriptor, SystemBuilder, etc.)
//! are defined in `basalt-core`. This module re-exports them and
//! provides the scheduling integration with [`Ecs`].

pub use basalt_core::{Phase, SystemAccess, SystemBuilder, SystemDescriptor, SystemRunner};
