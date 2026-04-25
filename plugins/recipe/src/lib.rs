//! Recipe plugin — crafting table interaction and recipe event handling.
//!
//! Opens the 3x3 crafting table window on right-click via
//! [`PlayerInteractEvent`] and drops crafting-grid contents at the
//! player's feet when the window closes (covers manual close, ESC,
//! and disconnect). Recipe matching itself runs through the
//! `CraftingRecipeMatchedEvent` pipeline.

use basalt_api::prelude::*;
use basalt_api::world::block;

/// Handles crafting table interaction and crafting-window cleanup.
///
/// Handlers:
/// - [`PlayerInteractEvent`] (Process, priority 0): opens the crafting
///   table window on right-click and cancels the event.
/// - [`ContainerClosedEvent`] (Post, priority 0): drops every
///   non-empty crafting grid slot as an item entity at the player's
///   position when a crafting table window closes.
pub struct RecipePlugin;

impl Plugin for RecipePlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "recipe",
            version: "0.1.0",
            author: Some("Basalt"),
            dependencies: &[],
        }
    }

    fn on_enable(&self, registrar: &mut PluginRegistrar) {
        // Open crafting table on right-click (Process stage, cancels event)
        registrar.on::<PlayerInteractEvent>(Stage::Process, 0, |event, ctx| {
            if block::is_crafting_table(event.block_state) {
                ctx.containers().open_crafting_table(
                    event.position.x,
                    event.position.y,
                    event.position.z,
                );
                event.cancel();
            }
        });

        // Drop crafting grid contents at the player's position when a
        // crafting table window closes. The server has already
        // captured `event.crafting_grid_state` and reset the grid to
        // 2x2; this handler just spawns the item entities.
        registrar.on::<ContainerClosedEvent>(Stage::Post, 0, |event, ctx| {
            if event.inventory_type != InventoryType::Crafting {
                return;
            }
            let Some(slots) = &event.crafting_grid_state else {
                return;
            };
            let (px, py, pz) = ctx.player().position();
            let drop_x = px as i32;
            let drop_y = py as i32 + 1;
            let drop_z = pz as i32;
            for slot in slots.iter() {
                if let Some(item_id) = slot.item_id {
                    ctx.entities().spawn_dropped_item(
                        drop_x,
                        drop_y,
                        drop_z,
                        item_id,
                        slot.item_count,
                    );
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use basalt_api::components::BlockPosition;
    use basalt_testkit::PluginTestHarness;

    use super::*;

    #[test]
    fn interact_crafting_table_cancels_event() {
        let mut harness = PluginTestHarness::new();
        harness.register(RecipePlugin);

        let mut event = PlayerInteractEvent {
            position: BlockPosition { x: 5, y: 64, z: 3 },
            block_state: block::CRAFTING_TABLE,
            direction: 1,
            sequence: 1,
            cancelled: false,
        };

        harness.dispatch(&mut event);
        assert!(
            event.is_cancelled(),
            "interact on crafting table should cancel"
        );
    }

    #[test]
    fn interact_non_crafting_table_does_not_cancel() {
        let mut harness = PluginTestHarness::new();
        harness.register(RecipePlugin);

        let mut event = PlayerInteractEvent {
            position: BlockPosition { x: 5, y: 64, z: 3 },
            block_state: block::STONE,
            direction: 1,
            sequence: 1,
            cancelled: false,
        };

        harness.dispatch(&mut event);
        assert!(!event.is_cancelled(), "interact on stone should not cancel");
    }

    #[test]
    fn interact_crafting_table_queues_open_response() {
        let mut harness = PluginTestHarness::new();
        harness.register(RecipePlugin);

        let mut event = PlayerInteractEvent {
            position: BlockPosition { x: 5, y: 64, z: 3 },
            block_state: block::CRAFTING_TABLE,
            direction: 1,
            sequence: 1,
            cancelled: false,
        };

        let result = harness.dispatch(&mut event);
        assert!(
            !result.is_empty(),
            "should produce an OpenCraftingTable response"
        );
    }

    #[test]
    fn interact_non_crafting_table_produces_no_response() {
        let mut harness = PluginTestHarness::new();
        harness.register(RecipePlugin);

        let mut event = PlayerInteractEvent {
            position: BlockPosition { x: 5, y: 64, z: 3 },
            block_state: block::STONE,
            direction: 1,
            sequence: 1,
            cancelled: false,
        };

        let result = harness.dispatch(&mut event);
        assert!(
            result.is_empty(),
            "stone interact should produce no responses"
        );
    }

    fn populated_grid() -> [basalt_api::types::Slot; 9] {
        let mut slots: [basalt_api::types::Slot; 9] =
            std::array::from_fn(|_| basalt_api::types::Slot::empty());
        slots[0] = basalt_api::types::Slot::new(43, 1);
        slots[2] = basalt_api::types::Slot::new(280, 4);
        slots
    }

    #[test]
    fn crafting_close_drops_grid_items() {
        let mut harness = PluginTestHarness::new();
        harness.register(RecipePlugin);

        let mut event = ContainerClosedEvent {
            window_id: 1,
            inventory_type: InventoryType::Crafting,
            backing: ContainerBacking::Block {
                position: BlockPosition { x: 5, y: 64, z: 3 },
            },
            reason: CloseReason::Manual,
            viewer_count: 0,
            crafting_grid_state: Some(populated_grid()),
        };

        let result = harness.dispatch(&mut event);
        assert!(result.has_spawn_dropped_item(43, 1));
        assert!(result.has_spawn_dropped_item(280, 4));
    }

    #[test]
    fn crafting_close_with_no_snapshot_drops_nothing() {
        let mut harness = PluginTestHarness::new();
        harness.register(RecipePlugin);

        let mut event = ContainerClosedEvent {
            window_id: 1,
            inventory_type: InventoryType::Crafting,
            backing: ContainerBacking::Block {
                position: BlockPosition { x: 5, y: 64, z: 3 },
            },
            reason: CloseReason::Manual,
            viewer_count: 0,
            crafting_grid_state: None,
        };

        let result = harness.dispatch(&mut event);
        assert!(!result.has_any_spawn_dropped_item());
    }

    #[test]
    fn non_crafting_close_drops_nothing_even_with_snapshot() {
        let mut harness = PluginTestHarness::new();
        harness.register(RecipePlugin);

        // Snapshot is populated but the inventory type is a chest —
        // the plugin must ignore it.
        let mut event = ContainerClosedEvent {
            window_id: 1,
            inventory_type: InventoryType::Generic9x3,
            backing: ContainerBacking::Block {
                position: BlockPosition { x: 5, y: 64, z: 3 },
            },
            reason: CloseReason::Manual,
            viewer_count: 0,
            crafting_grid_state: Some(populated_grid()),
        };

        let result = harness.dispatch(&mut event);
        assert!(!result.has_any_spawn_dropped_item());
    }
}
