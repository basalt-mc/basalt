//! Crafting events: grid changes and output clicks.

/// The contents of a crafting grid have changed.
///
/// Fired when a player places or removes an item in any crafting slot.
/// The recipe matching system listens for this event to compute the
/// crafting output. The crafting player is available via `ctx.player()`.
#[derive(Debug, Clone)]
pub struct CraftingGridChangedEvent {
    /// Item IDs in the 9 grid slots (`None` for empty slots).
    /// For a 2x2 grid, only indices 0-3 are populated.
    pub grid: [Option<i32>; 9],
    /// Grid dimension: 2 for inventory crafting, 3 for crafting table.
    pub grid_size: u8,
}
crate::game_event!(CraftingGridChangedEvent);

/// A player clicked the crafting output slot to collect the result.
///
/// Cancellable — a Validate handler can prevent the craft (e.g.,
/// permissions, anti-cheat). The crafting player is available via
/// `ctx.player()`.
#[derive(Debug, Clone)]
pub struct CraftingOutputClickedEvent {
    /// Item ID of the crafting result.
    pub result_id: i32,
    /// Stack count of the crafting result.
    pub result_count: i32,
    /// Whether the player shift-clicked (craft all).
    pub shift_click: bool,
    /// Whether this event has been cancelled by a Validate handler.
    pub cancelled: bool,
}
crate::game_cancellable_event!(CraftingOutputClickedEvent);

#[cfg(test)]
mod tests {
    use basalt_events::Event;

    use super::*;

    #[test]
    fn grid_changed_not_cancellable() {
        let mut event = CraftingGridChangedEvent {
            grid: [None; 9],
            grid_size: 3,
        };
        event.cancel(); // no-op
        assert!(!event.is_cancelled());
    }

    #[test]
    fn output_clicked_cancellation() {
        let mut event = CraftingOutputClickedEvent {
            result_id: 1,
            result_count: 1,
            shift_click: false,
            cancelled: false,
        };
        assert!(!event.is_cancelled());
        event.cancel();
        assert!(event.is_cancelled());
    }

    #[test]
    fn event_routing() {
        use basalt_events::{BusKind, EventRouting};
        assert_eq!(CraftingGridChangedEvent::BUS, BusKind::Game);
        assert_eq!(CraftingOutputClickedEvent::BUS, BusKind::Game);
    }
}
