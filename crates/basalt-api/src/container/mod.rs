//! Container types and builder, re-exported from basalt-core.
//!
//! Provides [`Container`] (reusable template value), [`ContainerBuilder`]
//! for fluent construction, [`InventoryType`] for identifying Minecraft
//! window types, and [`ContainerBacking`] for distinguishing virtual vs.
//! block-backed containers.

pub use basalt_core::container::{Container, ContainerBacking, ContainerBuilder, InventoryType};
