//! Minimal in-memory recipe registry for tests.
//!
//! [`MockRecipeRegistry`] implements [`RecipeRegistryHandle`] so test
//! code can exercise the `RecipeRegistrar` wrapper and `PluginRegistrar`
//! without depending on the concrete `basalt_recipes::RecipeRegistry`.

use crate::recipes::handle::RecipeRegistryHandle;
use crate::recipes::{OwnedShapedRecipe, OwnedShapelessRecipe, Recipe, RecipeId};

/// Minimal in-memory recipe registry for test harnesses.
///
/// All mutation and query methods behave identically to the production
/// `RecipeRegistry` — this is a thin `Vec`-backed store.
pub struct MockRecipeRegistry {
    /// Registered shaped recipes.
    shaped: Vec<OwnedShapedRecipe>,
    /// Registered shapeless recipes.
    shapeless: Vec<OwnedShapelessRecipe>,
}

impl MockRecipeRegistry {
    /// Creates an empty mock registry.
    pub fn new() -> Self {
        Self {
            shaped: Vec::new(),
            shapeless: Vec::new(),
        }
    }

    /// Returns the number of registered shaped recipes.
    pub fn shaped_count(&self) -> usize {
        self.shaped.len()
    }

    /// Returns the number of registered shapeless recipes.
    pub fn shapeless_count(&self) -> usize {
        self.shapeless.len()
    }

    /// Returns `true` if a recipe with the given id is registered.
    pub fn contains(&self, id: &RecipeId) -> bool {
        self.shaped.iter().any(|r| &r.id == id) || self.shapeless.iter().any(|r| &r.id == id)
    }

    /// Registers a shaped recipe (raw, no event dispatch).
    pub fn add_shaped(&mut self, recipe: OwnedShapedRecipe) {
        self.shaped.push(recipe);
    }

    /// Registers a shapeless recipe (raw, no event dispatch).
    pub fn add_shapeless(&mut self, recipe: OwnedShapelessRecipe) {
        self.shapeless.push(recipe);
    }
}

impl Default for MockRecipeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl RecipeRegistryHandle for MockRecipeRegistry {
    fn add_shaped(&mut self, recipe: OwnedShapedRecipe) {
        self.shaped.push(recipe);
    }

    fn add_shapeless(&mut self, recipe: OwnedShapelessRecipe) {
        self.shapeless.push(recipe);
    }

    fn remove_by_id(&mut self, id: &RecipeId) -> Option<Recipe> {
        if let Some(idx) = self.shaped.iter().position(|r| &r.id == id) {
            return Some(Recipe::Shaped(self.shaped.remove(idx)));
        }
        if let Some(idx) = self.shapeless.iter().position(|r| &r.id == id) {
            return Some(Recipe::Shapeless(self.shapeless.remove(idx)));
        }
        None
    }

    fn remove_by_result(&mut self, result_id: i32) -> Vec<RecipeId> {
        let mut removed = Vec::new();
        self.shaped.retain(|r| {
            if r.result_id == result_id {
                removed.push(r.id.clone());
                false
            } else {
                true
            }
        });
        self.shapeless.retain(|r| {
            if r.result_id == result_id {
                removed.push(r.id.clone());
                false
            } else {
                true
            }
        });
        removed
    }

    fn clear(&mut self) -> Vec<RecipeId> {
        let mut removed = Vec::with_capacity(self.shaped.len() + self.shapeless.len());
        removed.extend(self.shaped.drain(..).map(|r| r.id));
        removed.extend(self.shapeless.drain(..).map(|r| r.id));
        removed
    }

    fn contains(&self, id: &RecipeId) -> bool {
        self.shaped.iter().any(|r| &r.id == id) || self.shapeless.iter().any(|r| &r.id == id)
    }

    fn find_by_id(&self, id: &RecipeId) -> Option<Recipe> {
        if let Some(r) = self.shaped.iter().find(|r| &r.id == id) {
            return Some(Recipe::Shaped(r.clone()));
        }
        if let Some(r) = self.shapeless.iter().find(|r| &r.id == id) {
            return Some(Recipe::Shapeless(r.clone()));
        }
        None
    }

    fn shaped_count(&self) -> usize {
        self.shaped.len()
    }

    fn shapeless_count(&self) -> usize {
        self.shapeless.len()
    }
}
