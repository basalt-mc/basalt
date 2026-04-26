//! Vanilla Minecraft recipe data and runtime matching registry.
//!
//! This crate provides two layers:
//!
//! - **`generated`** — codegen'd static recipe data for all 1557 vanilla
//!   recipes (shaped and shapeless). These are `&'static` slices with zero
//!   runtime allocation.
//! - **`registry`** — a [`RecipeRegistry`] that indexes the generated data
//!   for efficient recipe matching against crafting grid contents.
//!
//! # Design
//!
//! - **Static lifetime**: all recipe data lives in read-only program memory.
//! - **No serde**: recipes are baked in at compile time via codegen, not
//!   loaded from files at runtime.

pub mod generated;
mod handle;
pub mod registry;

pub use generated::{SHAPED_RECIPES, SHAPELESS_RECIPES, ShapedRecipe, ShapelessRecipe};
pub use registry::RecipeRegistry;

// Re-export from basalt-api so existing `basalt_recipes::Recipe` paths
// in basalt-server keep working.
pub use basalt_api::recipes::{OwnedShapedRecipe, OwnedShapelessRecipe, Recipe, RecipeId};
