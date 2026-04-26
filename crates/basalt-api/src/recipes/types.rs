//! Owned recipe data types for plugin interaction.
//!
//! These types are the plugin-facing representations of crafting recipes.
//! They own their data on the heap (unlike the static codegen'd types in
//! `basalt-recipes::generated`) and are used in event payloads, registry
//! trait methods, and plugin registration.

use crate::recipes::id::RecipeId;

/// An owned shaped crafting recipe for plugin-registered custom recipes.
///
/// Unlike the static [`basalt_recipes::generated::ShapedRecipe`] which uses
/// `&'static` slices, this type owns its pattern data on the heap.
/// The `result_count` is `i32` (not `u8`) for flexibility in plugin recipes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedShapedRecipe {
    /// Stable identifier — must be unique across the registry.
    pub id: RecipeId,
    /// Grid width (1-3 for standard crafting table recipes).
    pub width: u8,
    /// Grid height (1-3 for standard crafting table recipes).
    pub height: u8,
    /// Flat grid of ingredient item IDs in row-major order.
    ///
    /// Length must equal `width * height`. `None` means the slot must be
    /// empty; `Some(id)` means the slot requires that item state ID.
    pub pattern: Vec<Option<i32>>,
    /// The item state ID of the crafted result.
    pub result_id: i32,
    /// How many items are produced per craft.
    pub result_count: i32,
}

/// An owned shapeless crafting recipe for plugin-registered custom recipes.
///
/// Unlike the static [`basalt_recipes::generated::ShapelessRecipe`] which uses
/// `&'static` slices, this type owns its ingredient list on the heap.
/// The `result_count` is `i32` (not `u8`) for flexibility in plugin recipes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedShapelessRecipe {
    /// Stable identifier — must be unique across the registry.
    pub id: RecipeId,
    /// Unordered set of required ingredient item state IDs, sorted ascending.
    ///
    /// Must be kept sorted for correct matching. Duplicates are allowed.
    pub ingredients: Vec<i32>,
    /// The item state ID of the crafted result.
    pub result_id: i32,
    /// How many items are produced per craft.
    pub result_count: i32,
}

/// A crafting recipe of either shape.
///
/// Used by event types and the registry's removal API to surface a recipe
/// regardless of its underlying shape kind. Plugin handlers match on the
/// variant when they need shape-specific access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recipe {
    /// A grid-pattern shaped recipe.
    Shaped(OwnedShapedRecipe),
    /// An unordered shapeless recipe.
    Shapeless(OwnedShapelessRecipe),
}

impl Recipe {
    /// Returns the recipe's stable identifier.
    pub fn id(&self) -> &RecipeId {
        match self {
            Self::Shaped(r) => &r.id,
            Self::Shapeless(r) => &r.id,
        }
    }

    /// Returns the item state ID of the crafted result.
    pub fn result_id(&self) -> i32 {
        match self {
            Self::Shaped(r) => r.result_id,
            Self::Shapeless(r) => r.result_id,
        }
    }

    /// Returns how many items are produced per craft.
    pub fn result_count(&self) -> i32 {
        match self {
            Self::Shaped(r) => r.result_count,
            Self::Shapeless(r) => r.result_count,
        }
    }
}
