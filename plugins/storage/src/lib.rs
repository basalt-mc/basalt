//! Storage plugin for chunk persistence.
//!
//! Persists modified chunks to disk after block changes. Disabling
//! this plugin means zero disk I/O on block changes — useful for
//! lobby servers with read-only worlds.

use basalt_api::prelude::*;

/// Persists chunks to disk after block modifications.
///
/// - **Post BlockBrokenEvent**: persists the affected chunk
/// - **Post BlockPlacedEvent**: persists the affected chunk
///
/// Uses priority 10 to run after BlockPlugin's Post handlers
/// (priority 0), ensuring the block change is committed before
/// persistence.
pub struct StoragePlugin;

impl Plugin for StoragePlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "storage",
            version: "0.1.0",
            author: Some("Basalt"),
            dependencies: &["block"],
        }
    }

    fn on_enable(&self, registrar: &mut PluginRegistrar) {
        registrar.on::<BlockBrokenEvent>(Stage::Post, 10, |event, ctx| {
            ctx.persist_chunk(event.x >> 4, event.z >> 4);
        });

        registrar.on::<BlockPlacedEvent>(Stage::Post, 10, |event, ctx| {
            ctx.persist_chunk(event.x >> 4, event.z >> 4);
        });
    }
}

#[cfg(test)]
mod tests {
    use basalt_test_utils::PluginTestHarness;
    use basalt_types::Uuid;

    use super::*;

    #[test]
    fn storage_persists_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let world = std::sync::Arc::new(basalt_world::World::new(42, dir.path()));

        let mut harness = PluginTestHarness::with_world(world);
        // Block plugin sets the block, storage plugin persists
        harness.register(basalt_plugin_block::BlockPlugin);
        harness.register(StoragePlugin);

        let mut event = BlockPlacedEvent {
            x: 5,
            y: 100,
            z: 3,
            block_state: basalt_world::block::STONE,
            sequence: 1,
            player_uuid: Uuid::default(),
            cancelled: false,
        };

        harness.dispatch(&mut event);

        // Verify persisted — fresh world should see the block
        let world2 = basalt_world::World::new(42, dir.path());
        assert_eq!(world2.get_block(5, 100, 3), basalt_world::block::STONE);
    }
}
