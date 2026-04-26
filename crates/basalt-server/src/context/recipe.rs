//! [`RecipeContext`] implementation for [`ServerContext`].
//!
//! Mutations queue a [`Response::UnlockRecipe`] /
//! [`Response::LockRecipe`] for the game loop to commit. Reads
//! (`has`, `unlocked`) hit the snapshot of the player's
//! [`KnownRecipes`] captured at context construction.

use basalt_api::context::{RecipeContext, Response, UnlockReason};
use basalt_recipes::RecipeId;

use super::ServerContext;

impl RecipeContext for ServerContext {
    fn unlock(&self, id: &RecipeId, reason: UnlockReason) {
        self.responses.push(Response::UnlockRecipe {
            recipe_id: id.clone(),
            reason,
        });
    }

    fn lock(&self, id: &RecipeId) {
        self.responses.push(Response::LockRecipe {
            recipe_id: id.clone(),
        });
    }

    fn has(&self, id: &RecipeId) -> bool {
        self.known_recipes.has(id)
    }

    fn unlocked(&self) -> Vec<RecipeId> {
        self.known_recipes
            .iter()
            .map(|(id, _)| id.clone())
            .collect()
    }
}
