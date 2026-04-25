//! Crafting events: grid changes, recipe matching, and craft execution.

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

#[cfg(test)]
mod tests {
    use basalt_events::{BusKind, Event, EventRouting};

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
}
