//! Test utilities for Basalt plugins and internal crates.
//!
//! Contains [`NoopContext`] (always available under `#[cfg(test)]` or
//! the `testing` feature) plus [`PluginTestHarness`] and
//! [`SystemTestContext`] (require the `testing` feature and the
//! `basalt-ecs` optional dependency).
//!
//! # Example
//!
//! ```ignore
//! let mut harness = PluginTestHarness::new();
//! harness.register(MyPlugin);
//!
//! let mut event = BlockBrokenEvent { position: BlockPosition { x: 5, y: 64, z: 3 }, ... };
//! let result = harness.dispatch(&mut event);
//! assert_eq!(result.len(), 2);
//! assert!(result.has_block_ack());
//! ```

mod mock_recipes;
mod mock_world;
mod noop;

#[cfg(feature = "testing")]
mod harness;

pub use mock_recipes::MockRecipeRegistry;
pub use mock_world::MockWorld;
pub use noop::NoopContext;

#[cfg(feature = "testing")]
pub use harness::{DispatchResult, PluginTestHarness, SystemTestContext};
