//! Crafting events: grid changes, recipe matching, craft execution,
//! and recipe-registry lifecycle.

use basalt_core::context::UnlockReason;
use basalt_recipes::{Recipe, RecipeId};
use basalt_types::Slot;

/// The contents of a crafting grid have changed.
///
/// Fired at the **Post** stage on the **game** bus whenever a player
/// places, removes, or rearranges an item in any crafting slot. This
/// is a pure notification — the result of the new grid is computed
/// separately and surfaced through [`CraftingRecipeMatchedEvent`] /
/// [`CraftingRecipeClearedEvent`].
///
/// The crafting player is available via `ctx.player()`.
#[derive(Debug, Clone)]
pub struct CraftingGridChangedEvent {
    /// Item IDs in the 9 grid slots (`None` for empty slots).
    /// For a 2x2 grid, only indices 0-3 are populated.
    pub grid: [Option<i32>; 9],
    /// Grid dimension: 2 for inventory crafting, 3 for crafting table.
    pub grid_size: u8,
}
crate::game_event!(CraftingGridChangedEvent);

/// A recipe was matched against the current crafting grid contents.
///
/// Fired at **Process + Post** stages on the **game** bus after the
/// server has resolved a matching recipe for the grid. Plugins can
/// **mutate `result`** at the Process stage (priority-ordered) to:
/// - augment the result (bonus count, custom NBT, applied enchantments)
/// - **deny the craft** by setting `result` to [`Slot::empty()`] —
///   the player will see no result appear in slot 0
///
/// After dispatch the server reads back `event.result` and writes it
/// to the player's `CraftingGrid.output`, then syncs slot 0 to the
/// client. Post listeners observe the final (post-mutation) result.
#[derive(Debug, Clone)]
pub struct CraftingRecipeMatchedEvent {
    /// Item IDs in the 9 grid slots that produced the match
    /// (`None` for empty slots).
    pub grid: [Option<i32>; 9],
    /// Grid dimension: 2 for inventory crafting, 3 for crafting table.
    pub grid_size: u8,
    /// The crafting result. **Mutable at Process** — plugins layer
    /// modifications by handler priority. Setting this to
    /// [`Slot::empty()`] hides the result from the player.
    pub result: Slot,
}
crate::game_event!(CraftingRecipeMatchedEvent);

/// The current crafting grid no longer matches any recipe.
///
/// Fired at the **Post** stage on the **game** bus only on the
/// transition `matched → unmatched` (i.e. the previous tick had a
/// non-empty result, this tick has none). Useful for plugins that
/// want to react when a result disappears (UI hints, achievements
/// for "almost crafted X").
#[derive(Debug, Clone)]
pub struct CraftingRecipeClearedEvent {
    /// Grid dimension: 2 for inventory crafting, 3 for crafting table.
    pub grid_size: u8,
}
crate::game_event!(CraftingRecipeClearedEvent);

/// A player is about to take a crafting result (cancellable).
///
/// Fired at the **Validate** stage on the **game** bus when a player
/// clicks the crafting output slot — both for normal clicks and the
/// initial click of a shift-click batch. Cancelling the event aborts
/// the craft entirely (no consumption, no result transfer).
///
/// For shift-click batches, [`CraftingShiftClickBatchEvent`] fires
/// immediately after this event (if not cancelled here) to allow
/// plugins to cap the batch size.
///
/// The crafting player is available via `ctx.player()`.
#[derive(Debug, Clone)]
pub struct CraftingPreCraftEvent {
    /// The result the player is about to receive.
    pub result: Slot,
    /// Whether the player shift-clicked (batch craft).
    pub is_shift_click: bool,
    /// Whether this event has been cancelled by a Validate handler.
    pub cancelled: bool,
}
crate::game_cancellable_event!(CraftingPreCraftEvent);

/// A successful craft has been performed.
///
/// Fired at the **Post** stage on the **game** bus exactly **once
/// per crafted unit**. For a normal click, fires once. For a
/// shift-click batch, fires N times (one per loop iteration). The
/// canonical hook for stats / achievements / logging.
#[derive(Debug, Clone)]
pub struct CraftingCraftedEvent {
    /// Snapshot of the grid contents **before** ingredient
    /// consumption for this craft. Index 0..9 corresponds to grid
    /// slot indices.
    pub consumed: [Slot; 9],
    /// The result that was delivered to the player.
    pub produced: Slot,
}
crate::game_event!(CraftingCraftedEvent);

/// A shift-click batch craft is about to begin (cancellable).
///
/// Fired at the **Validate** stage on the **game** bus immediately
/// after [`CraftingPreCraftEvent`] when the player shift-clicks the
/// crafting output. Plugins can cancel the entire batch, or **lower
/// `max_count`** to cap the number of iterations (e.g. anti-grief
/// limit "max 16 crafted per shift-click"). Increasing `max_count`
/// has no effect — the natural inventory-space cap still applies.
#[derive(Debug, Clone)]
pub struct CraftingShiftClickBatchEvent {
    /// The result the player will receive on each iteration.
    pub result: Slot,
    /// Maximum number of crafts to perform. **Mutable at Validate**
    /// — plugins lower this to cap the batch. Initial value is
    /// `u32::MAX` (the loop is naturally capped by available
    /// inventory space).
    pub max_count: u32,
    /// Whether this event has been cancelled by a Validate handler.
    pub cancelled: bool,
}
crate::game_cancellable_event!(CraftingShiftClickBatchEvent);

/// A plugin is about to register a recipe (cancellable).
///
/// Fired at the **Validate** stage on the **game** bus when a plugin
/// calls `RecipeRegistrar::add_shaped` / `add_shapeless` from inside
/// `Plugin::on_enable`. Cancellation aborts the registration —
/// [`RecipeRegisteredEvent`] is **not** fired and the registry is
/// left untouched.
///
/// Useful for permission gating ("only `recipe-admin` may register
/// recipes") and compatibility checks ("a recipe with this id
/// already exists, refuse").
///
/// Fires during plugin loading, **before** any player exists. The
/// dispatch context (`ctx.player()`) returns sentinel data — handlers
/// must rely on the event payload, not the context.
#[derive(Debug, Clone)]
pub struct RecipeRegisterEvent {
    /// The recipe being registered.
    pub recipe: Recipe,
    /// Whether this event has been cancelled by a Validate handler.
    pub cancelled: bool,
}
crate::game_cancellable_event!(RecipeRegisterEvent);

/// A recipe has been registered with the runtime registry.
///
/// Fired at the **Post** stage on the **game** bus after a successful
/// (i.e. non-cancelled) call to `RecipeRegistrar::add_shaped` or
/// `add_shapeless`. Useful for plugins that index recipes (recipe
/// book UI, search, dependency tracking, analytics).
///
/// Fires during plugin loading; see [`RecipeRegisterEvent`] for the
/// context contract.
#[derive(Debug, Clone)]
pub struct RecipeRegisteredEvent {
    /// Stable identifier of the newly registered recipe.
    pub recipe_id: RecipeId,
}
crate::game_event!(RecipeRegisteredEvent);

/// A recipe has been removed from the runtime registry.
///
/// Fired at the **Post** stage on the **game** bus once per removed
/// recipe — including each entry removed by a single
/// `remove_by_result` call or by `clear`. Useful for plugins that
/// maintain a derived index of the registry.
///
/// Fires during plugin loading; see [`RecipeRegisterEvent`] for the
/// context contract.
#[derive(Debug, Clone)]
pub struct RecipeUnregisteredEvent {
    /// Stable identifier of the recipe that was removed.
    pub recipe_id: RecipeId,
}
crate::game_event!(RecipeUnregisteredEvent);

/// A recipe has been unlocked for the current player.
///
/// Fired at the **Post** stage on the **game** bus after the player's
/// `KnownRecipes` component records the recipe and the
/// `Recipe Book Add` packet has been queued. The crafting player is
/// available via `ctx.player()`.
#[derive(Debug, Clone)]
pub struct RecipeUnlockedEvent {
    /// Stable identifier of the unlocked recipe.
    pub recipe_id: RecipeId,
    /// Why the unlock happened — auto-discovery, manual grant, or
    /// initial-join starter set.
    pub reason: UnlockReason,
}
crate::game_event!(RecipeUnlockedEvent);

/// A recipe has been locked for the current player.
///
/// Fired at the **Post** stage on the **game** bus after the player's
/// `KnownRecipes` component drops the recipe and the
/// `Recipe Book Remove` packet has been queued. The crafting player
/// is available via `ctx.player()`.
#[derive(Debug, Clone)]
pub struct RecipeLockedEvent {
    /// Stable identifier of the locked recipe.
    pub recipe_id: RecipeId,
}
crate::game_event!(RecipeLockedEvent);

/// A player is about to auto-fill a recipe from the recipe book
/// (cancellable).
///
/// Fired at the **Validate** stage on the **game** bus after the
/// server has resolved the recipe and pre-checked that the player's
/// inventory has enough ingredients, but **before** any item is
/// moved. Cancelling the event aborts the auto-fill — the grid stays
/// untouched and `RecipeBookFilledEvent` is not dispatched. Plugins
/// use it for permission gating (admin-only recipes, anti-grief
/// rate-limits, gated unlocks).
///
/// The crafting player is available via `ctx.player()`.
#[derive(Debug, Clone)]
pub struct RecipeBookFillRequestEvent {
    /// Stable identifier of the recipe the player clicked.
    pub recipe_id: RecipeId,
    /// Whether the player shift-clicked (asking for the largest
    /// possible batch). The Phase 2 server implementation always
    /// behaves as if `false` — `true` is plumbed through but
    /// silently degrades for now (Phase 3 will add real stacking).
    pub make_all: bool,
    /// Whether this event has been cancelled by a Validate handler.
    pub cancelled: bool,
}
crate::game_cancellable_event!(RecipeBookFillRequestEvent);

/// A player auto-filled a recipe from the recipe book.
///
/// Fired at the **Post** stage on the **game** bus after the
/// inventory has been drained and the grid populated. The standard
/// match cycle (`CraftingRecipeMatchedEvent` etc.) has already run
/// at this point, so `ctx.player()`'s grid reflects the new state.
#[derive(Debug, Clone)]
pub struct RecipeBookFilledEvent {
    /// Stable identifier of the filled recipe.
    pub recipe_id: RecipeId,
    /// Whether the original request was a shift-click.
    pub make_all: bool,
}
crate::game_event!(RecipeBookFilledEvent);

#[cfg(test)]
mod tests {
    use crate::events::{BusKind, Event, EventRouting};

    use super::*;

    fn empty_grid() -> [Option<i32>; 9] {
        [None; 9]
    }

    fn empty_slots() -> [Slot; 9] {
        std::array::from_fn(|_| Slot::empty())
    }

    #[test]
    fn grid_changed_not_cancellable() {
        let mut event = CraftingGridChangedEvent {
            grid: empty_grid(),
            grid_size: 3,
        };
        event.cancel();
        assert!(!event.is_cancelled());
        assert_eq!(CraftingGridChangedEvent::BUS, BusKind::Game);
    }

    #[test]
    fn recipe_matched_carries_mutable_result() {
        let mut event = CraftingRecipeMatchedEvent {
            grid: empty_grid(),
            grid_size: 3,
            result: Slot::new(1, 4),
        };
        event.result = Slot::empty();
        assert!(event.result.item_id.is_none());
        // not cancellable
        event.cancel();
        assert!(!event.is_cancelled());
        assert_eq!(CraftingRecipeMatchedEvent::BUS, BusKind::Game);
    }

    #[test]
    fn recipe_cleared_not_cancellable() {
        let mut event = CraftingRecipeClearedEvent { grid_size: 2 };
        event.cancel();
        assert!(!event.is_cancelled());
        assert_eq!(CraftingRecipeClearedEvent::BUS, BusKind::Game);
    }

    #[test]
    fn pre_craft_cancellation() {
        let mut event = CraftingPreCraftEvent {
            result: Slot::new(280, 4),
            is_shift_click: false,
            cancelled: false,
        };
        assert!(!event.is_cancelled());
        event.cancel();
        assert!(event.is_cancelled());
        assert_eq!(CraftingPreCraftEvent::BUS, BusKind::Game);
    }

    #[test]
    fn crafted_carries_consumed_and_produced() {
        let mut consumed = empty_slots();
        consumed[0] = Slot::new(17, 1);
        let event = CraftingCraftedEvent {
            consumed,
            produced: Slot::new(280, 4),
        };
        assert_eq!(event.consumed[0].item_id, Some(17));
        assert_eq!(event.produced.item_count, 4);
        assert_eq!(CraftingCraftedEvent::BUS, BusKind::Game);
    }

    #[test]
    fn shift_click_batch_cap_and_cancel() {
        let mut event = CraftingShiftClickBatchEvent {
            result: Slot::new(280, 4),
            max_count: u32::MAX,
            cancelled: false,
        };
        event.max_count = 2;
        assert_eq!(event.max_count, 2);
        event.cancel();
        assert!(event.is_cancelled());
        assert_eq!(CraftingShiftClickBatchEvent::BUS, BusKind::Game);
    }

    fn sample_recipe(path: &str) -> Recipe {
        Recipe::Shaped(basalt_recipes::OwnedShapedRecipe {
            id: RecipeId::new("plugin", path),
            width: 1,
            height: 1,
            pattern: vec![Some(1)],
            result_id: 42,
            result_count: 1,
        })
    }

    #[test]
    fn recipe_register_cancellation() {
        let mut event = RecipeRegisterEvent {
            recipe: sample_recipe("magic_sword"),
            cancelled: false,
        };
        assert!(!event.is_cancelled());
        event.cancel();
        assert!(event.is_cancelled());
        assert_eq!(RecipeRegisterEvent::BUS, BusKind::Game);
    }

    #[test]
    fn recipe_registered_carries_id() {
        let mut event = RecipeRegisteredEvent {
            recipe_id: RecipeId::vanilla("crafting_table"),
        };
        // not cancellable
        event.cancel();
        assert!(!event.is_cancelled());
        assert_eq!(event.recipe_id.namespace, "minecraft");
        assert_eq!(RecipeRegisteredEvent::BUS, BusKind::Game);
    }

    #[test]
    fn recipe_unregistered_carries_id() {
        let mut event = RecipeUnregisteredEvent {
            recipe_id: RecipeId::new("plugin", "obsolete"),
        };
        event.cancel();
        assert!(!event.is_cancelled());
        assert_eq!(event.recipe_id.path, "obsolete");
        assert_eq!(RecipeUnregisteredEvent::BUS, BusKind::Game);
    }

    #[test]
    fn recipe_unlocked_carries_id_and_reason() {
        let mut event = RecipeUnlockedEvent {
            recipe_id: RecipeId::vanilla("oak_planks"),
            reason: UnlockReason::AutoDiscovered,
        };
        // not cancellable
        event.cancel();
        assert!(!event.is_cancelled());
        assert_eq!(event.reason, UnlockReason::AutoDiscovered);
        assert_eq!(RecipeUnlockedEvent::BUS, BusKind::Game);
    }

    #[test]
    fn recipe_locked_carries_id() {
        let mut event = RecipeLockedEvent {
            recipe_id: RecipeId::new("plugin", "expired"),
        };
        event.cancel();
        assert!(!event.is_cancelled());
        assert_eq!(event.recipe_id.path, "expired");
        assert_eq!(RecipeLockedEvent::BUS, BusKind::Game);
    }

    #[test]
    fn fill_request_cancellation() {
        let mut event = RecipeBookFillRequestEvent {
            recipe_id: RecipeId::vanilla("oak_planks"),
            make_all: false,
            cancelled: false,
        };
        assert!(!event.is_cancelled());
        event.cancel();
        assert!(event.is_cancelled());
        assert_eq!(RecipeBookFillRequestEvent::BUS, BusKind::Game);
    }

    #[test]
    fn fill_request_carries_make_all() {
        let event = RecipeBookFillRequestEvent {
            recipe_id: RecipeId::vanilla("oak_planks"),
            make_all: true,
            cancelled: false,
        };
        assert!(event.make_all);
    }

    #[test]
    fn filled_carries_id_and_make_all() {
        let mut event = RecipeBookFilledEvent {
            recipe_id: RecipeId::vanilla("crafting_table"),
            make_all: false,
        };
        // not cancellable
        event.cancel();
        assert!(!event.is_cancelled());
        assert_eq!(RecipeBookFilledEvent::BUS, BusKind::Game);
    }
}
