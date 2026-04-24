//! Runtime recipe matching registry.
//!
//! [`RecipeRegistry`] combines the static vanilla recipe data from
//! [`super::generated`] with plugin-added custom recipes, and provides
//! efficient matching against crafting grid contents.

use crate::generated::{SHAPED_RECIPES, SHAPELESS_RECIPES};

/// An owned shaped crafting recipe for plugin-registered custom recipes.
///
/// Unlike the static [`super::generated::ShapedRecipe`] which uses
/// `&'static` slices, this type owns its pattern data on the heap.
/// The `result_count` is `i32` (not `u8`) for flexibility in plugin recipes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedShapedRecipe {
    /// Grid width (1-3 for standard crafting table recipes).
    pub width: u8,
    /// Grid height (1-3 for standard crafting table recipes).
    pub height: u8,
    /// Flat grid of ingredient item IDs in row-major order.
    ///
    /// Length must equal `width * height`. `None` means the slot must be
    /// empty; `Some(id)` means the slot requires that item state ID.
    pub pattern: Vec<Option<i32>>,
    /// The item state ID of the crafted result.
    pub result_id: i32,
    /// How many items are produced per craft.
    pub result_count: i32,
}

/// An owned shapeless crafting recipe for plugin-registered custom recipes.
///
/// Unlike the static [`super::generated::ShapelessRecipe`] which uses
/// `&'static` slices, this type owns its ingredient list on the heap.
/// The `result_count` is `i32` (not `u8`) for flexibility in plugin recipes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedShapelessRecipe {
    /// Unordered set of required ingredient item state IDs, sorted ascending.
    ///
    /// Must be kept sorted for correct matching. Duplicates are allowed.
    pub ingredients: Vec<i32>,
    /// The item state ID of the crafted result.
    pub result_id: i32,
    /// How many items are produced per craft.
    pub result_count: i32,
}

/// Indexes vanilla and custom recipes for efficient crafting grid matching.
///
/// Built at server startup from the static recipe data in
/// [`super::generated`], with support for adding and removing custom
/// plugin recipes at runtime. The [`match_grid`](RecipeRegistry::match_grid)
/// method tests a crafting grid against all registered recipes.
///
/// Shaped recipes are matched with automatic bounding-box extraction and
/// horizontal mirroring. Shapeless recipes are matched by comparing sorted
/// ingredient multisets.
pub struct RecipeRegistry {
    /// All registered shaped recipes (vanilla + custom).
    shaped: Vec<OwnedShapedRecipe>,
    /// All registered shapeless recipes (vanilla + custom).
    shapeless: Vec<OwnedShapelessRecipe>,
}

impl RecipeRegistry {
    /// Creates a registry pre-populated with all vanilla recipes.
    ///
    /// Converts the 1285 static shaped recipes and 272 static shapeless
    /// recipes from [`super::generated`] into owned heap copies.
    pub fn with_vanilla() -> Self {
        let shaped = SHAPED_RECIPES
            .iter()
            .map(|r| OwnedShapedRecipe {
                width: r.width,
                height: r.height,
                pattern: r.ingredients.to_vec(),
                result_id: r.result_id,
                result_count: i32::from(r.result_count),
            })
            .collect();

        let shapeless = SHAPELESS_RECIPES
            .iter()
            .map(|r| OwnedShapelessRecipe {
                ingredients: r.ingredients.to_vec(),
                result_id: r.result_id,
                result_count: i32::from(r.result_count),
            })
            .collect();

        Self { shaped, shapeless }
    }

    /// Creates an empty registry with no recipes.
    ///
    /// Useful for servers that want only plugin-defined custom recipes.
    pub fn empty() -> Self {
        Self {
            shaped: Vec::new(),
            shapeless: Vec::new(),
        }
    }

    /// Registers a custom shaped recipe.
    pub fn add_shaped(&mut self, recipe: OwnedShapedRecipe) {
        self.shaped.push(recipe);
    }

    /// Registers a custom shapeless recipe.
    ///
    /// The recipe's `ingredients` must be sorted ascending for correct
    /// matching. This method does not enforce sorting — the caller is
    /// responsible for providing pre-sorted ingredients.
    pub fn add_shapeless(&mut self, recipe: OwnedShapelessRecipe) {
        self.shapeless.push(recipe);
    }

    /// Removes all recipes (shaped and shapeless) that produce the given result.
    pub fn remove_by_result(&mut self, result_id: i32) {
        self.shaped.retain(|r| r.result_id != result_id);
        self.shapeless.retain(|r| r.result_id != result_id);
    }

    /// Removes all recipes from the registry.
    pub fn clear(&mut self) {
        self.shaped.clear();
        self.shapeless.clear();
    }

    /// Returns the number of registered shaped recipes.
    pub fn shaped_count(&self) -> usize {
        self.shaped.len()
    }

    /// Returns the number of registered shapeless recipes.
    pub fn shapeless_count(&self) -> usize {
        self.shapeless.len()
    }

    /// Matches a crafting grid against all registered recipes.
    ///
    /// The `grid` is a flat row-major array of item slots. `grid_size` is
    /// the width (and height) of the square grid (2 for inventory crafting,
    /// 3 for a crafting table).
    ///
    /// Returns `Some((result_id, result_count))` for the first matching
    /// recipe, or `None` if no recipe matches. Shaped recipes are tested
    /// first (with horizontal mirroring), then shapeless recipes.
    pub fn match_grid(&self, grid: &[Option<i32>], grid_size: u8) -> Option<(i32, i32)> {
        let gs = grid_size as usize;

        // Compute the bounding box of non-empty slots (empty grid → None).
        let (min_row, max_row, min_col, max_col) = bounding_box(grid, grid_size)?;

        let bb_width = max_col - min_col + 1;
        let bb_height = max_row - min_row + 1;

        // Shaped matching: compare extracted sub-grid against each recipe.
        for recipe in &self.shaped {
            if recipe.width as usize != bb_width || recipe.height as usize != bb_height {
                continue;
            }

            // Check normal orientation.
            if pattern_matches(
                grid,
                gs,
                min_row,
                min_col,
                &recipe.pattern,
                bb_width,
                bb_height,
            ) {
                return Some((recipe.result_id, recipe.result_count));
            }

            // Check horizontally mirrored orientation.
            let mirrored = mirror_pattern(&recipe.pattern, recipe.width, recipe.height);
            if pattern_matches(grid, gs, min_row, min_col, &mirrored, bb_width, bb_height) {
                return Some((recipe.result_id, recipe.result_count));
            }
        }

        // Shapeless matching: compare sorted ingredient multisets.
        let mut items: Vec<i32> = grid.iter().filter_map(|slot| *slot).collect();
        let item_count = items.len();
        items.sort_unstable();

        for recipe in &self.shapeless {
            if recipe.ingredients.len() != item_count {
                continue;
            }
            if recipe.ingredients == items {
                return Some((recipe.result_id, recipe.result_count));
            }
        }

        None
    }
}

/// Computes the axis-aligned bounding box of non-empty slots in a grid.
///
/// Returns `(min_row, max_row, min_col, max_col)` as zero-based indices,
/// or `None` if every slot in the grid is empty.
fn bounding_box(grid: &[Option<i32>], grid_size: u8) -> Option<(usize, usize, usize, usize)> {
    let gs = grid_size as usize;
    let mut min_row = gs;
    let mut max_row = 0;
    let mut min_col = gs;
    let mut max_col = 0;
    let mut found = false;

    for (i, slot) in grid.iter().enumerate() {
        if slot.is_some() {
            let row = i / gs;
            let col = i % gs;
            if !found {
                min_row = row;
                max_row = row;
                min_col = col;
                max_col = col;
                found = true;
            } else {
                min_row = min_row.min(row);
                max_row = max_row.max(row);
                min_col = min_col.min(col);
                max_col = max_col.max(col);
            }
        }
    }

    if found {
        Some((min_row, max_row, min_col, max_col))
    } else {
        None
    }
}

/// Checks whether the sub-grid at the given offset matches a recipe pattern.
///
/// Compares each cell in the bounding box region of the input grid against
/// the corresponding cell in the recipe pattern (both in row-major order).
fn pattern_matches(
    grid: &[Option<i32>],
    grid_size: usize,
    start_row: usize,
    start_col: usize,
    pattern: &[Option<i32>],
    width: usize,
    height: usize,
) -> bool {
    for row in 0..height {
        for col in 0..width {
            let grid_idx = (start_row + row) * grid_size + (start_col + col);
            let pat_idx = row * width + col;
            if grid[grid_idx] != pattern[pat_idx] {
                return false;
            }
        }
    }
    true
}

/// Horizontally flips a recipe pattern.
///
/// Each row is reversed independently. For example, a 3x1 pattern
/// `[A, B, C]` becomes `[C, B, A]`.
fn mirror_pattern(pattern: &[Option<i32>], width: u8, height: u8) -> Vec<Option<i32>> {
    let w = width as usize;
    let h = height as usize;
    let mut mirrored = Vec::with_capacity(w * h);

    for row in 0..h {
        for col in (0..w).rev() {
            mirrored.push(pattern[row * w + col]);
        }
    }

    mirrored
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_vanilla_counts() {
        let reg = RecipeRegistry::with_vanilla();
        assert_eq!(reg.shaped_count(), 1285);
        assert_eq!(reg.shapeless_count(), 272);
    }

    #[test]
    fn empty_registry_matches_nothing() {
        let reg = RecipeRegistry::empty();
        let grid = [
            Some(43),
            Some(43),
            None,
            Some(43),
            Some(43),
            None,
            None,
            None,
            None,
        ];
        assert_eq!(reg.match_grid(&grid, 3), None);
    }

    /// Oak planks (ID 43) in a 2x2 pattern produce a crafting table (result 314).
    #[test]
    fn match_crafting_table_3x3() {
        let reg = RecipeRegistry::with_vanilla();
        // 2x2 of oak planks in the top-left of a 3x3 grid.
        let grid = [
            Some(43),
            Some(43),
            None,
            Some(43),
            Some(43),
            None,
            None,
            None,
            None,
        ];
        let result = reg.match_grid(&grid, 3);
        assert!(result.is_some(), "should match 2x2 oak planks recipe");
        let (id, count) = result.unwrap();
        assert_eq!(id, 314);
        assert_eq!(count, 1);
    }

    /// Same 2x2 recipe in a 2x2 grid (inventory crafting).
    #[test]
    fn match_crafting_table_2x2() {
        let reg = RecipeRegistry::with_vanilla();
        let grid = [Some(43), Some(43), Some(43), Some(43)];
        let result = reg.match_grid(&grid, 2);
        assert!(result.is_some(), "should match 2x2 oak planks in 2x2 grid");
        let (id, count) = result.unwrap();
        assert_eq!(id, 314);
        assert_eq!(count, 1);
    }

    /// Place a 2x2 recipe in the bottom-right of a 3x3 grid.
    #[test]
    fn match_offset_in_3x3() {
        let reg = RecipeRegistry::with_vanilla();
        let grid = [
            None,
            None,
            None,
            None,
            Some(43),
            Some(43),
            None,
            Some(43),
            Some(43),
        ];
        let result = reg.match_grid(&grid, 3);
        assert!(result.is_some(), "should match 2x2 offset to bottom-right");
        let (id, count) = result.unwrap();
        assert_eq!(id, 314);
        assert_eq!(count, 1);
    }

    /// A custom asymmetric recipe is matched when the grid is mirrored.
    #[test]
    fn match_mirrored() {
        let mut reg = RecipeRegistry::empty();
        // Asymmetric 2x2 pattern:
        //   [A, B]
        //   [A, _]
        reg.add_shaped(OwnedShapedRecipe {
            width: 2,
            height: 2,
            pattern: vec![Some(100), Some(200), Some(100), None],
            result_id: 9999,
            result_count: 1,
        });

        // Place the mirrored pattern on the grid:
        //   [B, A]
        //   [_, A]
        let grid = [
            Some(200),
            Some(100),
            None,
            None,
            Some(100),
            None,
            None,
            None,
            None,
        ];
        let result = reg.match_grid(&grid, 3);
        assert!(result.is_some(), "mirrored pattern should match");
        assert_eq!(result.unwrap(), (9999, 1));
    }

    /// Shapeless recipe: items 4 + 839 in any grid position produce result 2.
    #[test]
    fn match_shapeless() {
        let reg = RecipeRegistry::with_vanilla();
        // Place items 4 and 839 in arbitrary positions on a 3x3 grid.
        let grid = [None, Some(839), None, None, None, None, Some(4), None, None];
        let result = reg.match_grid(&grid, 3);
        assert!(result.is_some(), "shapeless recipe [4, 839] -> 2");
        let (id, count) = result.unwrap();
        assert_eq!(id, 2);
        assert_eq!(count, 1);
    }

    /// A grid with items that form no known recipe.
    #[test]
    fn no_match() {
        let reg = RecipeRegistry::with_vanilla();
        // Arbitrary non-recipe arrangement.
        let grid = [
            Some(9999),
            None,
            None,
            None,
            Some(9998),
            None,
            None,
            None,
            Some(9997),
        ];
        assert_eq!(reg.match_grid(&grid, 3), None);
    }

    /// An entirely empty grid matches nothing.
    #[test]
    fn empty_grid() {
        let reg = RecipeRegistry::with_vanilla();
        let grid = [None; 9];
        assert_eq!(reg.match_grid(&grid, 3), None);
    }

    /// Adding then removing a recipe by result ID.
    #[test]
    fn remove_by_result() {
        let mut reg = RecipeRegistry::empty();
        reg.add_shaped(OwnedShapedRecipe {
            width: 1,
            height: 1,
            pattern: vec![Some(42)],
            result_id: 7777,
            result_count: 2,
        });

        let grid = [Some(42), None, None, None];
        assert!(reg.match_grid(&grid, 2).is_some());

        reg.remove_by_result(7777);
        assert_eq!(reg.match_grid(&grid, 2), None);
        assert_eq!(reg.shaped_count(), 0);
    }

    /// Clear all recipes, then add a custom one.
    #[test]
    fn clear_then_custom() {
        let mut reg = RecipeRegistry::with_vanilla();
        assert!(reg.shaped_count() > 0);
        assert!(reg.shapeless_count() > 0);

        reg.clear();
        assert_eq!(reg.shaped_count(), 0);
        assert_eq!(reg.shapeless_count(), 0);

        // Add a custom shapeless recipe.
        reg.add_shapeless(OwnedShapelessRecipe {
            ingredients: vec![1, 2],
            result_id: 5555,
            result_count: 3,
        });

        let grid = [Some(2), None, None, Some(1)];
        let result = reg.match_grid(&grid, 2);
        assert!(result.is_some(), "custom shapeless should match");
        assert_eq!(result.unwrap(), (5555, 3));

        // Vanilla recipe should no longer match.
        let vanilla_grid = [Some(43), Some(43), Some(43), Some(43)];
        assert_eq!(reg.match_grid(&vanilla_grid, 2), None);
    }

    /// Bounding box helper with various grid configurations.
    #[test]
    fn bounding_box_empty() {
        let grid: [Option<i32>; 9] = [None; 9];
        assert_eq!(bounding_box(&grid, 3), None);
    }

    #[test]
    fn bounding_box_single_item() {
        // Single item at center of 3x3 grid (index 4 = row 1, col 1).
        let mut grid = [None; 9];
        grid[4] = Some(1);
        assert_eq!(bounding_box(&grid, 3), Some((1, 1, 1, 1)));
    }

    #[test]
    fn bounding_box_top_left_corner() {
        let mut grid = [None; 9];
        grid[0] = Some(1);
        grid[1] = Some(2);
        grid[3] = Some(3);
        grid[4] = Some(4);
        assert_eq!(bounding_box(&grid, 3), Some((0, 1, 0, 1)));
    }

    #[test]
    fn bounding_box_bottom_right_corner() {
        let mut grid = [None; 9];
        grid[4] = Some(1);
        grid[5] = Some(2);
        grid[7] = Some(3);
        grid[8] = Some(4);
        assert_eq!(bounding_box(&grid, 3), Some((1, 2, 1, 2)));
    }

    #[test]
    fn bounding_box_full_row() {
        // Items on the middle row only.
        let mut grid = [None; 9];
        grid[3] = Some(10);
        grid[4] = Some(20);
        grid[5] = Some(30);
        assert_eq!(bounding_box(&grid, 3), Some((1, 1, 0, 2)));
    }

    #[test]
    fn bounding_box_diagonal() {
        // Items on the diagonal.
        let mut grid = [None; 9];
        grid[0] = Some(1);
        grid[8] = Some(2);
        assert_eq!(bounding_box(&grid, 3), Some((0, 2, 0, 2)));
    }

    /// Mirror helper produces a horizontally-flipped pattern.
    #[test]
    fn mirror_pattern_asymmetric() {
        let pattern = vec![Some(1), Some(2), Some(3), Some(4), None, Some(5)];
        let mirrored = mirror_pattern(&pattern, 3, 2);
        assert_eq!(
            mirrored,
            vec![Some(3), Some(2), Some(1), Some(5), None, Some(4)]
        );
    }

    /// Mirror of a symmetric pattern is identical.
    #[test]
    fn mirror_pattern_symmetric() {
        let pattern = vec![Some(1), Some(1), Some(1), Some(1)];
        let mirrored = mirror_pattern(&pattern, 2, 2);
        assert_eq!(mirrored, pattern);
    }

    /// A normal (non-mirrored) asymmetric recipe matches directly.
    #[test]
    fn match_asymmetric_normal_orientation() {
        let mut reg = RecipeRegistry::empty();
        reg.add_shaped(OwnedShapedRecipe {
            width: 2,
            height: 2,
            pattern: vec![Some(100), Some(200), Some(100), None],
            result_id: 8888,
            result_count: 1,
        });

        // Place the recipe exactly (not mirrored).
        let grid = [
            Some(100),
            Some(200),
            None,
            Some(100),
            None,
            None,
            None,
            None,
            None,
        ];
        let result = reg.match_grid(&grid, 3);
        assert!(result.is_some(), "normal orientation should match");
        assert_eq!(result.unwrap(), (8888, 1));
    }

    /// `remove_by_result` removes from both shaped and shapeless vecs.
    #[test]
    fn remove_by_result_both_types() {
        let mut reg = RecipeRegistry::empty();
        reg.add_shaped(OwnedShapedRecipe {
            width: 1,
            height: 1,
            pattern: vec![Some(1)],
            result_id: 42,
            result_count: 1,
        });
        reg.add_shapeless(OwnedShapelessRecipe {
            ingredients: vec![2, 3],
            result_id: 42,
            result_count: 1,
        });
        assert_eq!(reg.shaped_count(), 1);
        assert_eq!(reg.shapeless_count(), 1);

        reg.remove_by_result(42);
        assert_eq!(reg.shaped_count(), 0);
        assert_eq!(reg.shapeless_count(), 0);
    }
}
