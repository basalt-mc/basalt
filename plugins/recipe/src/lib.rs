//! Recipe plugin — crafting table interaction and recipe event handling.
//!
//! Opens the 3x3 crafting table window on right-click via
//! [`PlayerInteractEvent`]. Future extensions may add recipe validation,
//! custom recipe registration, or crafting permissions.

use basalt_api::prelude::*;
use basalt_api::world::block;

/// Handles crafting table interaction and recipe-related events.
///
/// Currently registers a single handler:
/// - [`PlayerInteractEvent`] (Process, priority 0): opens the crafting
///   table window when the player right-clicks a crafting table block,
///   and cancels the event to prevent block placement.
///
/// Recipe matching itself is performed by the game loop after it
/// dispatches [`CraftingGridChangedEvent`], using the shared
/// [`RecipeRegistry`](basalt_api::recipes::RecipeRegistry).
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
}
