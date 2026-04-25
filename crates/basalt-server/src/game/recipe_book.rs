//! Recipe-book server-side wiring: response handlers, on-join hook,
//! and `Recipe → RecipeDisplay` conversion.
//!
//! Phase 1 covers shaped + shapeless crafting recipes. Furnace,
//! stonecutter, and smithing displays are placeholders here — those
//! domains (#138 and follow-ups) construct their own `RecipeDisplay`s.

use std::collections::HashMap;

use basalt_api::events::{
    RecipeBookFillRequestEvent, RecipeBookFilledEvent, RecipeLockedEvent, RecipeUnlockedEvent,
};
use basalt_core::components::{CraftingGrid, Inventory, KnownRecipes, Position};
use basalt_core::context::UnlockReason;
use basalt_ecs::EntityId;
use basalt_events::Event;
use basalt_protocol::types::{RecipeBookEntry, RecipeDisplay, SlotDisplay};
use basalt_recipes::{Recipe, RecipeId};
use basalt_types::{Slot, Uuid};

use super::{GameLoop, OutputHandle};
use crate::messages::ServerOutput;

/// Item id of a vanilla crafting table — drawn next to crafting
/// recipes in the recipe book UI.
const CRAFTING_TABLE_ITEM_ID: i32 = 314;

/// Default recipe-book category for crafting recipes.
///
/// `crafting_misc` = `3` per the 1.21.4 `recipe_book_category`
/// registry. We pick a single bucket for Phase 1; per-recipe category
/// classification (building blocks / redstone / equipment) needs the
/// item registry and is deferred.
const CATEGORY_CRAFTING_MISC: i32 = 3;

impl GameLoop {
    /// Handles `Response::UnlockRecipe`.
    ///
    /// Resolves the source player, mutates their `KnownRecipes`,
    /// dispatches `RecipeUnlockedEvent`, and queues a single-entry
    /// `Recipe Book Add` S2C packet.
    pub(super) fn unlock_recipe(
        &mut self,
        source_uuid: Uuid,
        recipe_id: RecipeId,
        reason: UnlockReason,
    ) {
        let Some(eid) = self.find_by_uuid(source_uuid) else {
            return;
        };
        let Some(recipe) = self.recipes.find_by_id(&recipe_id) else {
            log::warn!(
                target: "basalt::recipes",
                "unlock_recipe: unknown recipe id {recipe_id}"
            );
            return;
        };
        let display = to_display(&recipe);

        let display_id = {
            let Some(known) = self.ecs.get_mut::<KnownRecipes>(eid) else {
                return;
            };
            // Skip if already unlocked — KnownRecipes::unlock returns
            // the existing display_id but we should not re-send the
            // RecipeBookAdd packet (the client already has the entry).
            if known.has(&recipe_id) {
                return;
            }
            known.unlock(recipe_id.clone())
        };

        let entry = RecipeBookEntry {
            display_id,
            display,
            group: 0,
            category: CATEGORY_CRAFTING_MISC,
            crafting_requirements: None,
            // 0x01 = notification (toast on add).
            flags: 0x01,
        };
        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::RecipeBookAdd {
                entries: vec![entry],
                replace: false,
            });
        });

        let (entity_id, username, yaw, pitch) = self.player_dispatch_args(eid);
        let ctx = self.make_context(source_uuid, entity_id, &username, yaw, pitch);
        let mut event = RecipeUnlockedEvent { recipe_id, reason };
        self.dispatch_event(&mut event, &ctx);
        self.process_responses(source_uuid, &ctx.drain_responses());
    }

    /// Handles `Response::LockRecipe`.
    pub(super) fn lock_recipe(&mut self, source_uuid: Uuid, recipe_id: RecipeId) {
        let Some(eid) = self.find_by_uuid(source_uuid) else {
            return;
        };

        let display_id = {
            let Some(known) = self.ecs.get_mut::<KnownRecipes>(eid) else {
                return;
            };
            match known.lock(&recipe_id) {
                Some(d) => d,
                None => return,
            }
        };

        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::RecipeBookRemove {
                display_ids: vec![display_id],
            });
        });

        let (entity_id, username, yaw, pitch) = self.player_dispatch_args(eid);
        let ctx = self.make_context(source_uuid, entity_id, &username, yaw, pitch);
        let mut event = RecipeLockedEvent { recipe_id };
        self.dispatch_event(&mut event, &ctx);
        self.process_responses(source_uuid, &ctx.drain_responses());
    }

    /// Handles `GameInput::PlaceRecipe` (the `Place Recipe` C2S
    /// packet) — sends a ghost-recipe reply and auto-fills the
    /// player's crafting grid with the recipe's ingredients.
    ///
    /// Auto-fill is atomic: it pre-checks that the inventory has all
    /// required ingredients before mutating anything. If the
    /// inventory is short, only the ghost reply is sent. Plugins can
    /// veto a fill at the [`RecipeBookFillRequestEvent`] (Validate)
    /// stage — the ghost still goes out (purely visual), but no
    /// items move. Existing grid contents are returned to the
    /// inventory before placement; if the inventory is full they're
    /// dropped at the player's feet, mirroring manual click-out.
    ///
    /// `make_all = true` is plumbed through but treated as `false`
    /// (single craft) for now — multi-craft stacking is a Phase 3
    /// follow-up.
    pub(super) fn handle_place_recipe(
        &mut self,
        source_uuid: Uuid,
        window_id: i32,
        display_id: i32,
        make_all: bool,
    ) {
        if make_all {
            log::trace!(
                target: "basalt::recipes",
                "PlaceRecipe make_all=true degraded to single craft (Phase 3 follow-up)"
            );
        }
        let Some(eid) = self.find_by_uuid(source_uuid) else {
            return;
        };
        let recipe_id = match self.ecs.get::<KnownRecipes>(eid) {
            Some(known) => match known.recipe_for_display(display_id) {
                Some(id) => id.clone(),
                None => return,
            },
            None => return,
        };
        let Some(recipe) = self.recipes.find_by_id(&recipe_id) else {
            return;
        };

        // Ghost reply — Phase 1 behaviour, fires whether or not
        // auto-fill succeeds so the visual preview is reliable.
        let display = to_display(&recipe);
        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::SendGhostRecipe { window_id, display });
        });

        // ── Auto-fill ──────────────────────────────────────
        let grid_size = self
            .ecs
            .get::<CraftingGrid>(eid)
            .map(|g| g.grid_size)
            .unwrap_or(2);
        let Some(plan) = build_placement_plan(&recipe, grid_size) else {
            return; // Recipe doesn't fit the open grid.
        };
        let requirements = aggregate_requirements(&plan);

        let drain = match self.ecs.get::<Inventory>(eid) {
            Some(inv) => match find_inventory_drain(inv, &requirements) {
                Some(d) => d,
                None => return, // Inventory is short — silent abort.
            },
            None => return,
        };

        // Validate hook — plugins can cancel here.
        let (entity_id, username, yaw, pitch) = self.player_dispatch_args(eid);
        let ctx = self.make_context(source_uuid, entity_id, &username, yaw, pitch);
        let mut request_event = RecipeBookFillRequestEvent {
            recipe_id: recipe_id.clone(),
            make_all,
            cancelled: false,
        };
        self.dispatch_event(&mut request_event, &ctx);
        self.process_responses(source_uuid, &ctx.drain_responses());
        if request_event.is_cancelled() {
            return;
        }

        // Snapshot the existing grid so we can return its contents
        // to the inventory after the drain.
        let grid_capacity = (grid_size as usize) * (grid_size as usize);
        let returns: Vec<Slot> = self
            .ecs
            .get::<CraftingGrid>(eid)
            .map(|g| {
                (0..grid_capacity)
                    .filter_map(|i| {
                        let s = &g.slots[i];
                        if s.item_id.is_some() {
                            Some(s.clone())
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Drain the inventory and re-insert returns. Returns that
        // don't fit overflow into the world as dropped items.
        let mut overflow: Vec<Slot> = Vec::new();
        if let Some(inv) = self.ecs.get_mut::<Inventory>(eid) {
            for (inv_slot, dec) in &drain {
                let s = &mut inv.slots[*inv_slot];
                s.item_count -= *dec;
                if s.item_count <= 0 {
                    *s = Slot::empty();
                }
            }
            for slot in returns {
                let item_id = slot.item_id.expect("filtered to non-empty above");
                if inv.try_insert(item_id, slot.item_count).is_none() {
                    overflow.push(slot);
                }
            }
        }
        if !overflow.is_empty() {
            let (px, py, pz) = self
                .ecs
                .get::<Position>(eid)
                .map(|p| (p.x as i32, p.y as i32, p.z as i32))
                .unwrap_or((0, 0, 0));
            for slot in overflow {
                if let Some(item_id) = slot.item_id {
                    self.spawn_item_entity(px, py + 1, pz, item_id, slot.item_count);
                }
            }
        }

        // Apply placements: clear unused grid slots first so any
        // leftovers from the previous match don't linger.
        if let Some(grid) = self.ecs.get_mut::<CraftingGrid>(eid) {
            for i in 0..grid_capacity {
                grid.slots[i] = Slot::empty();
            }
            for (grid_idx, item_id) in &plan {
                grid.slots[*grid_idx] = Slot::new(*item_id, 1);
            }
        }

        // Sync — full inventory window items + grid slots, then run
        // the match cycle so the result slot lights up.
        self.sync_full_inventory(eid);
        self.sync_crafting_grid_to_client(eid);
        self.run_crafting_match_cycle(source_uuid, eid);

        // Post hook — plugins observe the completed fill.
        let mut filled_event = RecipeBookFilledEvent {
            recipe_id,
            make_all,
        };
        self.dispatch_event(&mut filled_event, &ctx);
        self.process_responses(source_uuid, &ctx.drain_responses());
    }

    /// Sends every inventory slot to the client.
    ///
    /// Used after an auto-fill to make sure both the player-inventory
    /// window and the open container window (if any) reflect the new
    /// state, since the auto-fill may touch many slots at once.
    fn sync_full_inventory(&self, eid: EntityId) {
        let Some(inv) = self.ecs.get::<Inventory>(eid) else {
            return;
        };
        let slots = inv.to_protocol_slots();
        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::SyncInventory { slots });
        });
    }

    /// Pulls the dispatch args (entity id, username, yaw, pitch) for a
    /// player entity. Returns sentinels if any component is missing —
    /// dispatch goes through with stub values rather than skipping.
    fn player_dispatch_args(&self, eid: EntityId) -> (i32, String, f32, f32) {
        let entity_id = eid as i32;
        let username = self
            .ecs
            .get::<basalt_core::PlayerRef>(eid)
            .map_or_else(String::new, |p| p.username.clone());
        let (yaw, pitch) = self
            .ecs
            .get::<basalt_core::Rotation>(eid)
            .map_or((0.0, 0.0), |r| (r.yaw, r.pitch));
        (entity_id, username, yaw, pitch)
    }

    /// Sends the player's current recipe book on join.
    ///
    /// Always sends a `RecipeBookAdd { replace: true }` even when the
    /// player has no unlocked recipes — the 1.21.4 client expects the
    /// packet to initialize its book UI. Without it the client may
    /// display a stale book or refuse to open it.
    pub(super) fn send_initial_recipe_book(&self, eid: EntityId) {
        let entries = self
            .ecs
            .get::<KnownRecipes>(eid)
            .map(|known| {
                known
                    .iter()
                    .filter_map(|(id, display_id)| {
                        let recipe = self.recipes.find_by_id(id)?;
                        Some(RecipeBookEntry {
                            display_id,
                            display: to_display(&recipe),
                            group: 0,
                            category: CATEGORY_CRAFTING_MISC,
                            crafting_requirements: None,
                            flags: 0,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if let Some(handle) = self.ecs.get::<OutputHandle>(eid) {
            let _ = handle.tx.try_send(ServerOutput::RecipeBookAdd {
                entries,
                replace: true,
            });
        }
    }
}

/// Converts a [`Recipe`] into the wire-level [`RecipeDisplay`].
///
/// Shaped recipes preserve their `width` × `height` grid; ingredient
/// slots become `SlotDisplay::Item` (or `Empty` for `None`). Shapeless
/// recipes drop into `CraftingShapeless` with the same item-by-id
/// mapping. Other variants (Furnace, Stonecutter, Smithing) are
/// constructed by their domain plugins — out of scope for this phase.
pub fn to_display(recipe: &Recipe) -> RecipeDisplay {
    match recipe {
        Recipe::Shaped(r) => RecipeDisplay::CraftingShaped {
            width: i32::from(r.width),
            height: i32::from(r.height),
            ingredients: r.pattern.iter().map(slot_for_pattern).collect(),
            result: SlotDisplay::ItemStack {
                slot: Slot::new(r.result_id, r.result_count),
            },
            crafting_station: SlotDisplay::Item {
                item_id: CRAFTING_TABLE_ITEM_ID,
            },
        },
        Recipe::Shapeless(r) => RecipeDisplay::CraftingShapeless {
            ingredients: r
                .ingredients
                .iter()
                .map(|item_id| SlotDisplay::Item { item_id: *item_id })
                .collect(),
            result: SlotDisplay::ItemStack {
                slot: Slot::new(r.result_id, r.result_count),
            },
            crafting_station: SlotDisplay::Item {
                item_id: CRAFTING_TABLE_ITEM_ID,
            },
        },
    }
}

fn slot_for_pattern(slot: &Option<i32>) -> SlotDisplay {
    match slot {
        Some(id) => SlotDisplay::Item { item_id: *id },
        None => SlotDisplay::Empty,
    }
}

/// Builds the placement plan for auto-fill: a list of
/// `(grid_index, item_id)` pairs in row-major order.
///
/// Shaped recipes are placed top-left aligned in the target grid.
/// Returns `None` when the recipe doesn't fit the open grid (e.g.
/// a 3-wide recipe in a 2x2 inventory grid, or a shapeless recipe
/// with more ingredients than the grid has slots).
pub(super) fn build_placement_plan(recipe: &Recipe, grid_size: u8) -> Option<Vec<(usize, i32)>> {
    let g = grid_size as usize;
    match recipe {
        Recipe::Shaped(r) => {
            if r.width as usize > g || r.height as usize > g {
                return None;
            }
            let mut plan = Vec::new();
            for row in 0..(r.height as usize) {
                for col in 0..(r.width as usize) {
                    let pat_idx = row * (r.width as usize) + col;
                    if let Some(item_id) = r.pattern[pat_idx] {
                        let grid_idx = row * g + col;
                        plan.push((grid_idx, item_id));
                    }
                }
            }
            Some(plan)
        }
        Recipe::Shapeless(r) => {
            if r.ingredients.len() > g * g {
                return None;
            }
            Some(
                r.ingredients
                    .iter()
                    .enumerate()
                    .map(|(i, item_id)| (i, *item_id))
                    .collect(),
            )
        }
    }
}

/// Aggregates a placement plan into `(item_id, total_count)` pairs
/// so the inventory drain can be sized correctly per ingredient.
pub(super) fn aggregate_requirements(plan: &[(usize, i32)]) -> Vec<(i32, i32)> {
    let mut by_id: HashMap<i32, i32> = HashMap::new();
    for (_, item_id) in plan {
        *by_id.entry(*item_id).or_insert(0) += 1;
    }
    by_id.into_iter().collect()
}

/// Greedy search for a way to drain the inventory to satisfy the
/// given `(item_id, total_count)` requirements.
///
/// Walks hotbar (internal slots 0..9) then main (9..36), reserving
/// counts from matching stacks. Returns `Some(drain_plan)` when the
/// requirement is fully sourced, `None` otherwise. The drain plan is
/// `(internal_slot, decrement_count)` — apply each pair to the
/// inventory.
pub(super) fn find_inventory_drain(
    inv: &Inventory,
    requirements: &[(i32, i32)],
) -> Option<Vec<(usize, i32)>> {
    let mut drain = Vec::new();
    for (item_id, total_needed) in requirements {
        let mut remaining = *total_needed;
        for slot_idx in (0..9).chain(9..36) {
            if remaining == 0 {
                break;
            }
            let slot = &inv.slots[slot_idx];
            if slot.item_id == Some(*item_id) && slot.item_count > 0 {
                let take = slot.item_count.min(remaining);
                drain.push((slot_idx, take));
                remaining -= take;
            }
        }
        if remaining > 0 {
            return None;
        }
    }
    Some(drain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use basalt_recipes::{OwnedShapedRecipe, OwnedShapelessRecipe};

    fn shaped(width: u8, height: u8, pattern: Vec<Option<i32>>) -> Recipe {
        Recipe::Shaped(OwnedShapedRecipe {
            id: RecipeId::vanilla("test"),
            width,
            height,
            pattern,
            result_id: 879,
            result_count: 4,
        })
    }

    fn shapeless(ingredients: Vec<i32>) -> Recipe {
        Recipe::Shapeless(OwnedShapelessRecipe {
            id: RecipeId::vanilla("test_shapeless"),
            ingredients,
            result_id: 2,
            result_count: 1,
        })
    }

    #[test]
    fn to_display_shaped_preserves_dimensions_and_pattern() {
        let r = shaped(2, 1, vec![Some(43), Some(43)]);
        match to_display(&r) {
            RecipeDisplay::CraftingShaped {
                width,
                height,
                ingredients,
                result,
                crafting_station,
            } => {
                assert_eq!(width, 2);
                assert_eq!(height, 1);
                assert_eq!(ingredients.len(), 2);
                assert!(matches!(ingredients[0], SlotDisplay::Item { item_id: 43 }));
                assert!(matches!(result, SlotDisplay::ItemStack { .. }));
                assert!(
                    matches!(crafting_station, SlotDisplay::Item { item_id } if item_id == CRAFTING_TABLE_ITEM_ID)
                );
            }
            _ => panic!("expected CraftingShaped"),
        }
    }

    #[test]
    fn to_display_shaped_maps_none_to_empty() {
        let r = shaped(2, 2, vec![Some(1), None, None, Some(2)]);
        match to_display(&r) {
            RecipeDisplay::CraftingShaped { ingredients, .. } => {
                assert!(matches!(ingredients[0], SlotDisplay::Item { item_id: 1 }));
                assert!(matches!(ingredients[1], SlotDisplay::Empty));
                assert!(matches!(ingredients[2], SlotDisplay::Empty));
                assert!(matches!(ingredients[3], SlotDisplay::Item { item_id: 2 }));
            }
            _ => panic!("expected CraftingShaped"),
        }
    }

    #[test]
    fn to_display_shapeless_maps_each_ingredient() {
        let r = shapeless(vec![10, 20, 30]);
        match to_display(&r) {
            RecipeDisplay::CraftingShapeless { ingredients, .. } => {
                assert_eq!(ingredients.len(), 3);
                assert!(matches!(ingredients[0], SlotDisplay::Item { item_id: 10 }));
                assert!(matches!(ingredients[2], SlotDisplay::Item { item_id: 30 }));
            }
            _ => panic!("expected CraftingShapeless"),
        }
    }

    #[test]
    fn placement_plan_shaped_top_left_aligned_in_3x3() {
        // 1×2 stick recipe (one column, two rows) placed at top-left
        // of a 3x3 crafting-table grid → indices 0 (row 0, col 0) and 3 (row 1, col 0).
        let r = shaped(1, 2, vec![Some(43), Some(43)]);
        let plan = build_placement_plan(&r, 3).expect("fits");
        assert_eq!(plan, vec![(0, 43), (3, 43)]);
    }

    #[test]
    fn placement_plan_shaped_skips_pattern_holes() {
        // Asymmetric 2x2 with one empty slot in the corner.
        let r = shaped(2, 2, vec![Some(1), Some(2), Some(3), None]);
        let plan = build_placement_plan(&r, 3).expect("fits");
        // Row 0: indices 0, 1 → (0, 1), (1, 2)
        // Row 1: indices 3, 4 → (3, 3), and the None is skipped.
        assert_eq!(plan, vec![(0, 1), (1, 2), (3, 3)]);
    }

    #[test]
    fn placement_plan_shaped_3x3_in_2x2_returns_none() {
        let r = shaped(3, 3, vec![Some(1); 9]);
        assert_eq!(build_placement_plan(&r, 2), None);
    }

    #[test]
    fn placement_plan_shapeless_walks_slots_in_order() {
        let r = shapeless(vec![10, 20, 30]);
        let plan = build_placement_plan(&r, 3).expect("fits");
        assert_eq!(plan, vec![(0, 10), (1, 20), (2, 30)]);
    }

    #[test]
    fn placement_plan_shapeless_too_many_ingredients_returns_none() {
        // 5 ingredients can't fit in a 2x2 grid (4 slots).
        let r = shapeless(vec![1, 2, 3, 4, 5]);
        assert_eq!(build_placement_plan(&r, 2), None);
    }

    #[test]
    fn aggregate_requirements_counts_each_item() {
        let plan = vec![(0, 43), (1, 43), (3, 43), (4, 879)];
        let mut req = aggregate_requirements(&plan);
        req.sort_by_key(|(id, _)| *id);
        assert_eq!(req, vec![(43, 3), (879, 1)]);
    }

    #[test]
    fn drain_finds_single_slot_with_enough_count() {
        let mut inv = basalt_core::Inventory::empty();
        inv.slots[0] = Slot::new(43, 8);
        let plan = find_inventory_drain(&inv, &[(43, 3)]).expect("sufficient");
        assert_eq!(plan, vec![(0, 3)]);
    }

    #[test]
    fn drain_splits_across_slots_when_needed() {
        let mut inv = basalt_core::Inventory::empty();
        inv.slots[0] = Slot::new(43, 2);
        inv.slots[5] = Slot::new(43, 4);
        let plan = find_inventory_drain(&inv, &[(43, 5)]).expect("sufficient");
        // Hotbar 0 (2) is fully drained, then hotbar 5 contributes 3.
        assert_eq!(plan, vec![(0, 2), (5, 3)]);
    }

    #[test]
    fn drain_returns_none_when_inventory_short() {
        let mut inv = basalt_core::Inventory::empty();
        inv.slots[0] = Slot::new(43, 2);
        assert_eq!(find_inventory_drain(&inv, &[(43, 3)]), None);
    }

    #[test]
    fn drain_handles_mixed_ingredients() {
        let mut inv = basalt_core::Inventory::empty();
        inv.slots[0] = Slot::new(43, 4); // 4 oak planks
        inv.slots[10] = Slot::new(879, 2); // 2 sticks (in main inventory range)
        let plan = find_inventory_drain(&inv, &[(43, 2), (879, 1)]).expect("sufficient");
        assert!(plan.contains(&(0, 2)));
        assert!(plan.contains(&(10, 1)));
    }

    /// End-to-end auto-fill: 2 oak planks in hotbar slot 0 produce a
    /// stick recipe (1×2 oak planks → 4 sticks) when the player
    /// clicks it in the book. The grid lights up and the inventory
    /// is drained.
    #[test]
    fn auto_fill_stick_recipe_drains_inventory_and_fills_grid() {
        use basalt_recipes::{OwnedShapedRecipe, RecipeId};
        use basalt_types::Uuid;

        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        // Stick recipe: 1×2 oak planks → 4 sticks. id 1234 is unused
        // by vanilla so we can register it cleanly.
        let recipe_id = RecipeId::new("plugin", "test_sticks");
        let recipes =
            std::sync::Arc::get_mut(&mut game_loop.recipes).expect("registry is unique here");
        recipes.add_shaped(OwnedShapedRecipe {
            id: recipe_id.clone(),
            width: 1,
            height: 2,
            pattern: vec![Some(43), Some(43)],
            result_id: 879,
            result_count: 4,
        });
        game_loop.unlock_recipe(uuid, recipe_id.clone(), UnlockReason::Manual);

        // Stock the player with 2 oak planks in hotbar slot 0.
        let eid = game_loop.find_by_uuid(uuid).unwrap();
        if let Some(inv) = game_loop.ecs.get_mut::<Inventory>(eid) {
            inv.slots[0] = Slot::new(43, 2);
        }
        while rx.try_recv().is_ok() {}

        let display_id = game_loop
            .ecs
            .get::<KnownRecipes>(eid)
            .and_then(|k| k.display_id(&recipe_id))
            .unwrap();
        game_loop.handle_place_recipe(uuid, 0, display_id, false);

        let grid = game_loop.ecs.get::<CraftingGrid>(eid).unwrap();
        // 2x2 player-inventory grid: slot 0 + slot 2 (row 0, col 0 / row 1, col 0).
        assert_eq!(grid.slots[0].item_id, Some(43));
        assert_eq!(grid.slots[2].item_id, Some(43));
        assert!(grid.slots[1].is_empty());
        assert!(grid.slots[3].is_empty());
        assert_eq!(grid.output.item_id, Some(879));
        assert_eq!(grid.output.item_count, 4);

        let inv = game_loop.ecs.get::<Inventory>(eid).unwrap();
        assert!(inv.slots[0].is_empty(), "hotbar slot 0 should be drained");
    }

    /// When the player has no matching ingredients the auto-fill
    /// silently aborts — no grid mutation, no events fired beyond
    /// the ghost reply.
    #[test]
    fn auto_fill_aborts_silently_with_empty_inventory() {
        use basalt_recipes::{OwnedShapedRecipe, RecipeId};
        use basalt_types::Uuid;

        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let recipe_id = RecipeId::new("plugin", "test_sticks");
        let recipes =
            std::sync::Arc::get_mut(&mut game_loop.recipes).expect("registry is unique here");
        recipes.add_shaped(OwnedShapedRecipe {
            id: recipe_id.clone(),
            width: 1,
            height: 2,
            pattern: vec![Some(43), Some(43)],
            result_id: 879,
            result_count: 4,
        });
        game_loop.unlock_recipe(uuid, recipe_id.clone(), UnlockReason::Manual);
        while rx.try_recv().is_ok() {}

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        let display_id = game_loop
            .ecs
            .get::<KnownRecipes>(eid)
            .and_then(|k| k.display_id(&recipe_id))
            .unwrap();
        game_loop.handle_place_recipe(uuid, 0, display_id, false);

        let grid = game_loop.ecs.get::<CraftingGrid>(eid).unwrap();
        assert!(grid.slots[0].is_empty(), "grid should not be filled");
        assert!(grid.output.is_empty(), "no recipe matched");
    }

    /// Existing grid contents are returned to the inventory before
    /// the auto-fill places its own ingredients.
    #[test]
    fn auto_fill_returns_existing_grid_contents_to_inventory() {
        use basalt_recipes::{OwnedShapedRecipe, RecipeId};
        use basalt_types::Uuid;

        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let recipe_id = RecipeId::new("plugin", "test_sticks");
        let recipes =
            std::sync::Arc::get_mut(&mut game_loop.recipes).expect("registry is unique here");
        recipes.add_shaped(OwnedShapedRecipe {
            id: recipe_id.clone(),
            width: 1,
            height: 2,
            pattern: vec![Some(43), Some(43)],
            result_id: 879,
            result_count: 4,
        });
        game_loop.unlock_recipe(uuid, recipe_id.clone(), UnlockReason::Manual);

        // Stock the player with 2 oak planks AND seed the grid with
        // an unrelated diamond (id 56). After the fill, the diamond
        // should be back in the inventory.
        let eid = game_loop.find_by_uuid(uuid).unwrap();
        if let Some(inv) = game_loop.ecs.get_mut::<Inventory>(eid) {
            inv.slots[0] = Slot::new(43, 2);
        }
        if let Some(grid) = game_loop.ecs.get_mut::<CraftingGrid>(eid) {
            grid.slots[1] = Slot::new(56, 1);
        }
        while rx.try_recv().is_ok() {}

        let display_id = game_loop
            .ecs
            .get::<KnownRecipes>(eid)
            .and_then(|k| k.display_id(&recipe_id))
            .unwrap();
        game_loop.handle_place_recipe(uuid, 0, display_id, false);

        let inv = game_loop.ecs.get::<Inventory>(eid).unwrap();
        let has_diamond = inv
            .slots
            .iter()
            .any(|s| s.item_id == Some(56) && s.item_count == 1);
        assert!(
            has_diamond,
            "the pre-existing diamond should have been returned to the inventory"
        );
    }

    /// End-to-end test: unlock_recipe mutates KnownRecipes and queues
    /// a `RecipeBookAdd` packet to the player's output channel.
    #[test]
    fn unlock_recipe_updates_state_and_sends_packet() {
        use basalt_types::Uuid;

        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        // Drain the on-join packets so we only see the unlock dispatch.
        while rx.try_recv().is_ok() {}

        let id = RecipeId::vanilla("shaped_0");
        game_loop.unlock_recipe(uuid, id.clone(), UnlockReason::Manual);

        let eid = game_loop.find_by_uuid(uuid).expect("player entity");
        let known = game_loop
            .ecs
            .get::<KnownRecipes>(eid)
            .expect("KnownRecipes attached on join");
        assert!(known.has(&id));
        assert_eq!(known.display_id(&id), Some(0));

        let mut saw_add = false;
        while let Ok(out) = rx.try_recv() {
            if let crate::messages::ServerOutput::RecipeBookAdd { entries, replace } = out {
                assert!(!replace, "per-recipe unlock uses replace=false");
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].display_id, 0);
                saw_add = true;
            }
        }
        assert!(saw_add, "unlock_recipe should queue a RecipeBookAdd");
    }

    /// unlock_recipe is a no-op when the recipe id isn't in the
    /// registry — the player's KnownRecipes stays empty.
    #[test]
    fn unlock_recipe_unknown_id_is_noop() {
        use basalt_types::Uuid;
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let _rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        game_loop.unlock_recipe(
            uuid,
            RecipeId::new("plugin", "does_not_exist"),
            UnlockReason::Manual,
        );

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        let known = game_loop.ecs.get::<KnownRecipes>(eid).unwrap();
        assert_eq!(known.len(), 0);
    }

    /// unlock_recipe is idempotent — a second call for the same id
    /// does not allocate a new display_id and does not queue a duplicate
    /// packet.
    #[test]
    fn unlock_recipe_idempotent() {
        use basalt_types::Uuid;
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let id = RecipeId::vanilla("shaped_0");
        game_loop.unlock_recipe(uuid, id.clone(), UnlockReason::Manual);
        // Drain the first add.
        while rx.try_recv().is_ok() {}

        // Second call — should be a no-op.
        game_loop.unlock_recipe(uuid, id, UnlockReason::Manual);

        let mut saw_add = false;
        while let Ok(out) = rx.try_recv() {
            if matches!(out, crate::messages::ServerOutput::RecipeBookAdd { .. }) {
                saw_add = true;
            }
        }
        assert!(
            !saw_add,
            "second unlock for same recipe must not queue another packet"
        );
    }

    /// lock_recipe removes from KnownRecipes and queues a
    /// `RecipeBookRemove` packet with the previously-allocated display_id.
    #[test]
    fn lock_recipe_after_unlock_sends_remove() {
        use basalt_types::Uuid;
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let id = RecipeId::vanilla("shaped_0");
        game_loop.unlock_recipe(uuid, id.clone(), UnlockReason::Manual);
        while rx.try_recv().is_ok() {}

        game_loop.lock_recipe(uuid, id.clone());

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        let known = game_loop.ecs.get::<KnownRecipes>(eid).unwrap();
        assert!(!known.has(&id));

        let mut saw_remove = false;
        while let Ok(out) = rx.try_recv() {
            if let crate::messages::ServerOutput::RecipeBookRemove { display_ids } = out {
                assert_eq!(display_ids, vec![0]);
                saw_remove = true;
            }
        }
        assert!(
            saw_remove,
            "lock_recipe should queue a RecipeBookRemove with the removed display_id"
        );
    }

    /// `handle_place_recipe` resolves the per-player `display_id` and
    /// queues a `SendGhostRecipe` reply with the recipe's display.
    #[test]
    fn place_recipe_sends_ghost_recipe_for_known_display_id() {
        use basalt_types::Uuid;

        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        let id = RecipeId::vanilla("shaped_0");
        game_loop.unlock_recipe(uuid, id, UnlockReason::Manual);
        // Drain the unlock-related packets (RecipeBookAdd) so we
        // see only the ghost reply afterwards.
        while rx.try_recv().is_ok() {}

        // Display id 0 was assigned by KnownRecipes::unlock above.
        game_loop.handle_place_recipe(uuid, 0, 0, false);

        let mut saw_ghost = false;
        while let Ok(out) = rx.try_recv() {
            if let crate::messages::ServerOutput::SendGhostRecipe { window_id, .. } = out {
                assert_eq!(window_id, 0);
                saw_ghost = true;
            }
        }
        assert!(
            saw_ghost,
            "place_recipe with known display_id should queue SendGhostRecipe"
        );
    }

    /// `handle_place_recipe` is a no-op when the `display_id` was
    /// never allocated for this player.
    #[test]
    fn place_recipe_unknown_display_id_is_noop() {
        use basalt_types::Uuid;
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);
        while rx.try_recv().is_ok() {}

        game_loop.handle_place_recipe(uuid, 0, 9999, false);

        let saw_ghost = std::iter::from_fn(|| rx.try_recv().ok())
            .any(|o| matches!(o, crate::messages::ServerOutput::SendGhostRecipe { .. }));
        assert!(
            !saw_ghost,
            "unknown display_id must not queue a ghost reply"
        );
    }

    /// On player join, the server sends an empty
    /// `RecipeBookAdd { replace: true }` so the client initialises its
    /// recipe-book UI even when no recipes are unlocked.
    #[test]
    fn join_sends_initial_replace_recipe_book() {
        use basalt_types::Uuid;
        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();
        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let mut found = false;
        while let Ok(out) = rx.try_recv() {
            if let crate::messages::ServerOutput::RecipeBookAdd { entries, replace } = out
                && replace
                && entries.is_empty()
            {
                found = true;
            }
        }
        assert!(
            found,
            "expected RecipeBookAdd {{ replace: true, entries: [] }} on join"
        );
    }

    /// A Validate-stage handler that cancels `RecipeBookFillRequestEvent`
    /// blocks the auto-fill — the inventory is untouched and the grid
    /// stays empty. The ghost reply still goes out (purely visual).
    #[test]
    fn auto_fill_validate_cancellation_blocks_inventory_drain() {
        use basalt_api::events::RecipeBookFillRequestEvent;
        use basalt_recipes::{OwnedShapedRecipe, RecipeId};
        use basalt_types::Uuid;

        let (mut game_loop, game_tx, _io_rx) = super::super::tests::test_game_loop();

        // Register a Validate handler that cancels every fill.
        game_loop
            .bus
            .on::<RecipeBookFillRequestEvent, basalt_api::context::ServerContext>(
                basalt_events::Stage::Validate,
                0,
                |event, _| event.cancel(),
            );

        let uuid = Uuid::from_bytes([1; 16]);
        let mut rx = super::super::tests::connect_player(&mut game_loop, &game_tx, uuid, 1);

        let recipe_id = RecipeId::new("plugin", "test_sticks");
        let recipes =
            std::sync::Arc::get_mut(&mut game_loop.recipes).expect("registry is unique here");
        recipes.add_shaped(OwnedShapedRecipe {
            id: recipe_id.clone(),
            width: 1,
            height: 2,
            pattern: vec![Some(43), Some(43)],
            result_id: 879,
            result_count: 4,
        });
        game_loop.unlock_recipe(uuid, recipe_id.clone(), UnlockReason::Manual);

        let eid = game_loop.find_by_uuid(uuid).unwrap();
        if let Some(inv) = game_loop.ecs.get_mut::<Inventory>(eid) {
            inv.slots[0] = Slot::new(43, 2);
        }
        while rx.try_recv().is_ok() {}

        let display_id = game_loop
            .ecs
            .get::<KnownRecipes>(eid)
            .and_then(|k| k.display_id(&recipe_id))
            .unwrap();
        game_loop.handle_place_recipe(uuid, 0, display_id, false);

        let grid = game_loop.ecs.get::<CraftingGrid>(eid).unwrap();
        assert!(
            grid.slots[0].is_empty(),
            "cancellation should leave grid empty"
        );

        let inv = game_loop.ecs.get::<Inventory>(eid).unwrap();
        assert_eq!(
            inv.slots[0].item_count, 2,
            "cancellation should preserve the planks"
        );
    }
}
