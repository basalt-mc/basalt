//! Re-exports system types from `basalt-api` and integrates them
//! with the ECS scheduler.
//!
//! All system types (Phase, SystemDescriptor, SystemBuilder, etc.)
//! are defined in `basalt-api`. This module re-exports them and
//! provides the scheduling integration with [`Ecs`].

pub use basalt_api::system::{SystemAccess, SystemBuilder, SystemDescriptor, SystemRunner};

pub use basalt_api::components::Phase;
