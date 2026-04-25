//! Recipe-book server-side wiring: response handlers, on-join hook,
//! and `Recipe → RecipeDisplay` conversion.
//!
//! Phase 1 covers shaped + shapeless crafting recipes. Furnace,
//! stonecutter, and smithing displays are placeholders here — those
//! domains (#138 and follow-ups) construct their own `RecipeDisplay`s.

use basalt_api::events::{RecipeLockedEvent, RecipeUnlockedEvent};
use basalt_core::components::KnownRecipes;
use basalt_core::context::UnlockReason;
use basalt_ecs::EntityId;
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
    /// packet) by sending a ghost-recipe reply to the player's open
    /// crafting window.
    ///
    /// Resolves the protocol's per-player `display_id` to a
    /// [`RecipeId`] via the player's [`KnownRecipes`] (the reverse
    /// map is intentionally retained even after lock, so a stale
    /// click on a freshly-locked recipe still finds the entry), then
    /// looks the recipe up in the registry, builds a
    /// [`RecipeDisplay`], and queues a
    /// [`ServerOutput::SendGhostRecipe`].
    ///
    /// No items are moved on the server in Phase 1 — the ghost is a
    /// purely visual preview. Auto-fill (the
    /// `RecipeBookFillRequestEvent` / `RecipeBookFilledEvent` pair)
    /// is tracked as a follow-up issue.
    pub(super) fn handle_place_recipe(
        &mut self,
        source_uuid: Uuid,
        window_id: i32,
        display_id: i32,
    ) {
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
        let display = to_display(&recipe);
        self.send_to(eid, |tx| {
            let _ = tx.try_send(ServerOutput::SendGhostRecipe { window_id, display });
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
        game_loop.handle_place_recipe(uuid, 0, 0);

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

        game_loop.handle_place_recipe(uuid, 0, 9999);

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
}
