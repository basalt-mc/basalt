//! System registration for tick-based plugins.
//!
//! System plugins register a runner that executes each tick with
//! access to entities and the world via [`SystemContext`].

pub use basalt_core::{
    Phase, SystemAccess, SystemBuilder, SystemContext, SystemContextExt, SystemDescriptor,
    TickBudget,
};
