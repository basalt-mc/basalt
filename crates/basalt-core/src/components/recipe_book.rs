//! Per-player recipe-book state.
//!
//! Tracks which recipes a player has unlocked and the per-session
//! `display_id` mapping the protocol uses to reference them. The
//! mapping is session-scoped — display IDs are reassigned every time
//! the player connects.

use std::collections::{HashMap, HashSet};

use basalt_recipes::RecipeId;

use super::Component;

/// Set of recipes a player has unlocked, plus the protocol's
/// numeric `display_id` mapping.
///
/// The protocol uses an `i32` per recipe (allocated server-side, sent
/// in `Recipe Book Add` and referenced by `Recipe Book Remove` and
/// `Place Recipe`). Display IDs are stable for the lifetime of the
/// connection but are not persisted across sessions.
///
/// `unlock`/`lock` allocate / drop the `display_ids` mapping but
/// **keep** the reverse `by_display` lookup so a stale `Place Recipe`
/// packet from the client (e.g. arriving in the same tick as a remove
/// dispatch) can still resolve the recipe id rather than being silently
/// dropped.
#[derive(Debug, Default, Clone)]
pub struct KnownRecipes {
    /// Source of truth — the recipes the client should currently see.
    ids: HashSet<RecipeId>,
    /// Forward map: recipe id → display id. Trimmed on `lock`.
    display_ids: HashMap<RecipeId, i32>,
    /// Reverse map: display id → recipe id. Retained even after
    /// `lock` so stale incoming packets resolve cleanly.
    by_display: HashMap<i32, RecipeId>,
    /// Counter for the next display id to allocate.
    next_display_id: i32,
}

impl Component for KnownRecipes {}

impl KnownRecipes {
    /// Records the recipe as unlocked for this player.
    ///
    /// Allocates a new `display_id` if the recipe was not already
    /// known. Returns the recipe's `display_id` (existing or newly
    /// allocated) so the caller can include it in the
    /// `Recipe Book Add` S2C packet.
    pub fn unlock(&mut self, id: RecipeId) -> i32 {
        if !self.ids.insert(id.clone()) {
            // Already unlocked — return the existing display_id.
            return self.display_ids[&id];
        }
        let display_id = self.next_display_id;
        self.next_display_id += 1;
        self.display_ids.insert(id.clone(), display_id);
        self.by_display.insert(display_id, id);
        display_id
    }

    /// Removes the recipe from the unlocked set.
    ///
    /// Returns the `display_id` if the recipe was previously
    /// unlocked, otherwise `None`. The reverse `by_display` mapping
    /// is preserved so late-arriving `Place Recipe` packets can still
    /// resolve which recipe they referred to.
    pub fn lock(&mut self, id: &RecipeId) -> Option<i32> {
        if !self.ids.remove(id) {
            return None;
        }
        self.display_ids.remove(id)
    }

    /// Returns true if the recipe is unlocked for this player.
    pub fn has(&self, id: &RecipeId) -> bool {
        self.ids.contains(id)
    }

    /// Returns the display id assigned to the recipe, if any.
    ///
    /// Reads the forward map only — locked recipes return `None`
    /// even though the reverse map may still hold them.
    pub fn display_id(&self, id: &RecipeId) -> Option<i32> {
        self.display_ids.get(id).copied()
    }

    /// Resolves a `display_id` back to a recipe id.
    ///
    /// Used by Phase 2 to handle incoming `Place Recipe` packets.
    /// Returns the recipe even if it has since been locked — the
    /// caller decides whether to honour the request.
    pub fn recipe_for_display(&self, display_id: i32) -> Option<&RecipeId> {
        self.by_display.get(&display_id)
    }

    /// Returns the number of currently unlocked recipes.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Returns true if no recipe is unlocked.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Iterates the unlocked recipes paired with their display ids.
    ///
    /// Iteration order matches the `display_ids` map's iteration
    /// order, which is `HashMap`-defined (i.e. unspecified).
    pub fn iter(&self) -> impl Iterator<Item = (&RecipeId, i32)> {
        self.display_ids.iter().map(|(id, d)| (id, *d))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(path: &str) -> RecipeId {
        RecipeId::new("plugin", path)
    }

    #[test]
    fn unlock_allocates_sequential_display_ids() {
        let mut k = KnownRecipes::default();
        assert_eq!(k.unlock(id("a")), 0);
        assert_eq!(k.unlock(id("b")), 1);
        assert_eq!(k.unlock(id("c")), 2);
    }

    #[test]
    fn unlock_idempotent_returns_existing_display_id() {
        let mut k = KnownRecipes::default();
        let first = k.unlock(id("a"));
        let second = k.unlock(id("a"));
        assert_eq!(first, second);
        assert_eq!(k.len(), 1);
    }

    #[test]
    fn lock_returns_display_id_and_removes_forward_lookup() {
        let mut k = KnownRecipes::default();
        let display = k.unlock(id("a"));
        assert_eq!(k.lock(&id("a")), Some(display));
        assert!(!k.has(&id("a")));
        assert_eq!(k.display_id(&id("a")), None);
    }

    #[test]
    fn lock_keeps_reverse_lookup_for_stale_packets() {
        let mut k = KnownRecipes::default();
        let display = k.unlock(id("a"));
        k.lock(&id("a"));
        assert_eq!(k.recipe_for_display(display), Some(&id("a")));
    }

    #[test]
    fn lock_returns_none_when_unknown() {
        let mut k = KnownRecipes::default();
        assert_eq!(k.lock(&id("missing")), None);
    }

    #[test]
    fn display_ids_do_not_reuse_after_lock() {
        let mut k = KnownRecipes::default();
        k.unlock(id("a"));
        k.unlock(id("b"));
        k.lock(&id("a"));
        // Next unlock should keep allocating sequentially —
        // display_ids are session-stable, not reusable.
        assert_eq!(k.unlock(id("c")), 2);
    }

    #[test]
    fn iter_yields_only_unlocked_pairs() {
        let mut k = KnownRecipes::default();
        k.unlock(id("a"));
        k.unlock(id("b"));
        k.lock(&id("a"));

        let mut entries: Vec<_> = k.iter().map(|(id, d)| (id.clone(), d)).collect();
        entries.sort_by_key(|(_, d)| *d);
        assert_eq!(entries, vec![(id("b"), 1)]);
    }

    #[test]
    fn has_returns_true_only_for_unlocked() {
        let mut k = KnownRecipes::default();
        assert!(!k.has(&id("a")));
        k.unlock(id("a"));
        assert!(k.has(&id("a")));
        k.lock(&id("a"));
        assert!(!k.has(&id("a")));
    }
}
