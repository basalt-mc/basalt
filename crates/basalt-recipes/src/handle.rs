//! [`RecipeRegistryHandle`] implementation for [`RecipeRegistry`].
//!
//! Bridges the basalt-api trait with the concrete registry so that
//! `RecipeRegistrar` and `PluginRegistrar` can operate through a trait
//! object without knowing the concrete type.

use basalt_api::recipes::handle::RecipeRegistryHandle;
use basalt_api::recipes::{OwnedShapedRecipe, OwnedShapelessRecipe, Recipe, RecipeId};

use crate::registry::RecipeRegistry;

impl RecipeRegistryHandle for RecipeRegistry {
    fn add_shaped(&mut self, recipe: OwnedShapedRecipe) {
        RecipeRegistry::add_shaped(self, recipe);
    }

    fn add_shapeless(&mut self, recipe: OwnedShapelessRecipe) {
        RecipeRegistry::add_shapeless(self, recipe);
    }

    fn remove_by_id(&mut self, id: &RecipeId) -> Option<Recipe> {
        RecipeRegistry::remove_by_id(self, id)
    }

    fn remove_by_result(&mut self, result_id: i32) -> Vec<RecipeId> {
        RecipeRegistry::remove_by_result(self, result_id)
    }

    fn clear(&mut self) -> Vec<RecipeId> {
        RecipeRegistry::clear(self)
    }

    fn contains(&self, id: &RecipeId) -> bool {
        RecipeRegistry::contains(self, id)
    }

    fn find_by_id(&self, id: &RecipeId) -> Option<Recipe> {
        RecipeRegistry::find_by_id(self, id)
    }

    fn shaped_count(&self) -> usize {
        RecipeRegistry::shaped_count(self)
    }

    fn shapeless_count(&self) -> usize {
        RecipeRegistry::shapeless_count(self)
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
        let mut registry = RecipeRegistry::empty();
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
        let mut registry = RecipeRegistry::empty();
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
        let mut registry = RecipeRegistry::empty();
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
