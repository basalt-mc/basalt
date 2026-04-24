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
//! - **Zero dependencies**: pure Rust data structures and matching logic.
//! - **Static lifetime**: all recipe data lives in read-only program memory.
//! - **No serde**: recipes are baked in at compile time via codegen, not
//!   loaded from files at runtime.

pub mod generated;
pub mod registry;

pub use generated::{SHAPED_RECIPES, SHAPELESS_RECIPES, ShapedRecipe, ShapelessRecipe};
pub use registry::{OwnedShapedRecipe, OwnedShapelessRecipe, RecipeRegistry};
