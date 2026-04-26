//! Long-lived handle to the recipe registry.
//!
//! [`RecipeRegistryHandle`] abstracts the mutable recipe registry so
//! that [`RecipeRegistrar`](super::RecipeRegistrar) and
//! [`PluginRegistrar`](crate::PluginRegistrar) reference a trait object
//! instead of the concrete `basalt_recipes::RecipeRegistry`. This lets
//! basalt-api drop its direct dependency on basalt-recipes in a later
//! task once the data types move into basalt-api.

use crate::recipes::{OwnedShapedRecipe, OwnedShapelessRecipe, Recipe, RecipeId};

/// Long-lived handle to the recipe registry runtime.
///
/// Implemented by [`basalt_recipes::RecipeRegistry`] (production) and
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

// ── Impl for basalt_recipes::RecipeRegistry ─────────────────────────
//
// This impl lives in basalt-api temporarily. It moves to basalt-recipes
// in a later task (when basalt-recipes gains a basalt-api dep). Permitted
// by Rust's orphan rule because RecipeRegistryHandle is local to basalt-api.

impl RecipeRegistryHandle for basalt_recipes::RecipeRegistry {
    fn add_shaped(&mut self, recipe: OwnedShapedRecipe) {
        basalt_recipes::RecipeRegistry::add_shaped(self, recipe);
    }

    fn add_shapeless(&mut self, recipe: OwnedShapelessRecipe) {
        basalt_recipes::RecipeRegistry::add_shapeless(self, recipe);
    }

    fn remove_by_id(&mut self, id: &RecipeId) -> Option<Recipe> {
        basalt_recipes::RecipeRegistry::remove_by_id(self, id)
    }

    fn remove_by_result(&mut self, result_id: i32) -> Vec<RecipeId> {
        basalt_recipes::RecipeRegistry::remove_by_result(self, result_id)
    }

    fn clear(&mut self) -> Vec<RecipeId> {
        basalt_recipes::RecipeRegistry::clear(self)
    }

    fn contains(&self, id: &RecipeId) -> bool {
        basalt_recipes::RecipeRegistry::contains(self, id)
    }

    fn find_by_id(&self, id: &RecipeId) -> Option<Recipe> {
        basalt_recipes::RecipeRegistry::find_by_id(self, id)
    }

    fn shaped_count(&self) -> usize {
        basalt_recipes::RecipeRegistry::shaped_count(self)
    }

    fn shapeless_count(&self) -> usize {
        basalt_recipes::RecipeRegistry::shapeless_count(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that `RecipeRegistry` satisfies `RecipeRegistryHandle`
    /// and the trait methods delegate correctly to the concrete
    /// implementation.
    #[test]
    fn recipe_registry_implements_handle() {
        let mut registry = basalt_recipes::RecipeRegistry::empty();
        let handle: &mut dyn RecipeRegistryHandle = &mut registry;

        let id = RecipeId::new("test", "sword");
        assert!(!handle.contains(&id));

        handle.add_shaped(OwnedShapedRecipe {
            id: id.clone(),
            width: 1,
            height: 1,
            pattern: vec![Some(1)],
            result_id: 42,
            result_count: 1,
        });

        assert!(handle.contains(&id));
        assert_eq!(handle.shaped_count(), 1);
        assert_eq!(handle.shapeless_count(), 0);

        let recipe = handle.find_by_id(&id);
        assert!(matches!(recipe, Some(Recipe::Shaped(_))));
    }

    /// Verifies remove_by_id returns the removed recipe through the trait.
    #[test]
    fn handle_remove_by_id() {
        let mut registry = basalt_recipes::RecipeRegistry::empty();
        let handle: &mut dyn RecipeRegistryHandle = &mut registry;

        let id = RecipeId::new("test", "bread");
        handle.add_shapeless(OwnedShapelessRecipe {
            id: id.clone(),
            ingredients: vec![1, 2],
            result_id: 99,
            result_count: 1,
        });

        let removed = handle.remove_by_id(&id);
        assert!(matches!(removed, Some(Recipe::Shapeless(_))));
        assert!(!handle.contains(&id));
    }

    /// Verifies clear returns all ids through the trait.
    #[test]
    fn handle_clear() {
        let mut registry = basalt_recipes::RecipeRegistry::empty();
        let handle: &mut dyn RecipeRegistryHandle = &mut registry;

        handle.add_shaped(OwnedShapedRecipe {
            id: RecipeId::new("test", "a"),
            width: 1,
            height: 1,
            pattern: vec![Some(1)],
            result_id: 1,
            result_count: 1,
        });
        handle.add_shapeless(OwnedShapelessRecipe {
            id: RecipeId::new("test", "b"),
            ingredients: vec![2],
            result_id: 2,
            result_count: 1,
        });

        let removed = handle.clear();
        assert_eq!(removed.len(), 2);
        assert_eq!(handle.shaped_count(), 0);
        assert_eq!(handle.shapeless_count(), 0);
    }
}
