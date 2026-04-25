//! Component types and marker trait for the Entity Component System.
//!
//! These types define the data model for all game entities. They live
//! in `basalt-core` (not `basalt-ecs`) so that plugin crates can
//! reference them through `basalt-api` without depending on the ECS
//! storage engine.

mod container;
mod crafting;
mod identity;
mod inventory;
mod item;
mod recipe_book;
mod spatial;

pub use container::VirtualContainerSlots;
pub use crafting::CraftingGrid;
pub use identity::{EntityKind, Health, PlayerRef, Sneaking};
pub use inventory::Inventory;
pub use item::{DroppedItem, Lifetime, OpenContainer, PickupDelay};
pub use recipe_book::KnownRecipes;
pub use spatial::{BlockPosition, BoundingBox, ChunkPosition, Position, Rotation, Velocity};

/// Marker trait for component types stored in the ECS.
///
/// Components must be `Send + Sync + 'static` so they can be
/// accessed from the game loop thread and (in the future) from
/// parallel system threads via rayon.
pub trait Component: Send + Sync + 'static {}

/// A unique entity identifier.
///
/// Entities are just IDs — all data lives in component stores.
/// IDs are never reused within a server session.
pub type EntityId = u32;

/// Execution phase within a game loop tick.
///
/// Systems are grouped by phase and run in phase order.
/// Within a phase, independent systems can run in parallel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Phase {
    /// Drain input channels and convert to events/state.
    Input,
    /// Validation checks (permissions, anti-cheat). Can cancel.
    Validate,
    /// Active simulation: physics, AI, pathfinding, block updates.
    Simulate,
    /// Logical state mutations from event handlers.
    Process,
    /// Collect diffs, encode packets, push to output channels.
    Output,
    /// Side effects: logs, analytics, persistence.
    Post,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_ordering() {
        assert!(Phase::Input < Phase::Validate);
        assert!(Phase::Validate < Phase::Simulate);
        assert!(Phase::Simulate < Phase::Process);
        assert!(Phase::Process < Phase::Output);
        assert!(Phase::Output < Phase::Post);
    }
}
