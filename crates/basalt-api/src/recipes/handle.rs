//! Long-lived handle to the recipe registry.
//!
//! [`RecipeRegistryHandle`] abstracts the mutable recipe registry so
//! that [`RecipeRegistrar`](super::RecipeRegistrar) and
//! [`PluginRegistrar`](crate::PluginRegistrar) reference a trait object
//! instead of a concrete registry type. The concrete implementation
//! lives in `basalt-recipes`.

use crate::recipes::{OwnedShapedRecipe, OwnedShapelessRecipe, Recipe, RecipeId};

/// Long-lived handle to the recipe registry runtime.
///
/// Implemented by `basalt_recipes::RecipeRegistry` (production) and
/// mock types (tests). Plugins receive a `&mut dyn
/// RecipeRegistryHandle` indirectly via
/// [`RecipeRegistrar`](super::RecipeRegistrar).
///
/// Methods mirror the inherent API of `RecipeRegistry` — see each
/// method's doc for semantics.
pub trait RecipeRegistryHandle {
    /// Inserts a shaped recipe.
    ///
    /// The caller is responsible for id uniqueness. No duplicate
    /// checking is performed.
    fn add_shaped(&mut self, recipe: OwnedShapedRecipe);

    /// Inserts a shapeless recipe.
    ///
    /// The caller is responsible for sorting `recipe.ingredients`
    /// ascending and for id uniqueness.
    fn add_shapeless(&mut self, recipe: OwnedShapelessRecipe);

    /// Removes the recipe with the given id.
    ///
    /// Searches both shaped and shapeless registries. Returns the
    /// removed recipe or `None` if not present.
    fn remove_by_id(&mut self, id: &RecipeId) -> Option<Recipe>;

    /// Removes every recipe (shaped and shapeless) producing the given
    /// `result_id`.
    ///
    /// Returns the ids of the removed recipes in registry order —
    /// first shaped, then shapeless.
    fn remove_by_result(&mut self, result_id: i32) -> Vec<RecipeId>;

    /// Removes every recipe and returns their ids.
    ///
    /// Returned in registry order — first shaped, then shapeless.
    fn clear(&mut self) -> Vec<RecipeId>;

    /// Returns `true` if a recipe with the given id is registered.
    fn contains(&self, id: &RecipeId) -> bool;

    /// Returns a clone of the recipe with the given id, or `None`.
    ///
    /// Searches shaped recipes first then shapeless.
    fn find_by_id(&self, id: &RecipeId) -> Option<Recipe>;

    /// Returns the number of registered shaped recipes.
    fn shaped_count(&self) -> usize;

    /// Returns the number of registered shapeless recipes.
    fn shapeless_count(&self) -> usize;
}
