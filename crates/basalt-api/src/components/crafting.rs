//! Crafting grid component for player crafting state.

use basalt_types::Slot;

use crate::components::Component;

/// Tracks the contents of a player's crafting grid.
///
/// Used for both the 2x2 inventory crafting grid and the 3x3 crafting
/// table grid. The `grid_size` field determines which slots are active:
/// for a 2x2 grid, only `slots[0..4]` are used; for a 3x3 grid, all
/// nine slots are active.
///
/// The `output` slot holds the result computed by the recipe matching
/// system. It is read-only from the player's perspective — the server
/// sets it when the grid contents change.
#[derive(Debug, Clone)]
pub struct CraftingGrid {
    /// The 9 crafting input slots. For a 2x2 grid, only indices 0-3
    /// are used; indices 4-8 remain empty.
    pub slots: [Slot; 9],
    /// Grid dimension: 2 for the player inventory grid, 3 for a
    /// crafting table.
    pub grid_size: u8,
    /// The crafting output slot, set by the recipe matching system.
    pub output: Slot,
}

impl CraftingGrid {
    /// Creates an empty 2x2 crafting grid with no items.
    ///
    /// All nine slot positions are initialized to empty, and the output
    /// slot is empty. This is the default state for a player who has
    /// not interacted with any crafting interface.
    pub fn empty() -> Self {
        Self {
            slots: std::array::from_fn(|_| Slot::empty()),
            grid_size: 2,
            output: Slot::empty(),
        }
    }

    /// Clears all input slots and the output slot.
    ///
    /// The grid size is preserved — a 3x3 grid remains 3x3 after
    /// clearing. This is used when a player closes a crafting table
    /// window (items are returned to inventory, grid is reset).
    pub fn clear(&mut self) {
        for slot in &mut self.slots {
            *slot = Slot::empty();
        }
        self.output = Slot::empty();
    }
}

impl Component for CraftingGrid {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_returns_grid_size_two() {
        let grid = CraftingGrid::empty();
        assert_eq!(grid.grid_size, 2);
    }

    #[test]
    fn empty_has_all_empty_slots() {
        let grid = CraftingGrid::empty();
        for slot in &grid.slots {
            assert!(slot.is_empty());
        }
        assert!(grid.output.is_empty());
    }

    #[test]
    fn clear_resets_slots_but_keeps_grid_size() {
        let mut grid = CraftingGrid::empty();
        grid.grid_size = 3;
        grid.slots[0] = Slot::new(1, 1);
        grid.slots[4] = Slot::new(2, 5);
        grid.output = Slot::new(3, 1);

        grid.clear();

        assert_eq!(grid.grid_size, 3);
        for slot in &grid.slots {
            assert!(slot.is_empty());
        }
        assert!(grid.output.is_empty());
    }
}
