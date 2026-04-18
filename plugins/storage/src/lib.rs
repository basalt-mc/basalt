//! Storage plugin for chunk persistence configuration.
//!
//! Chunk persistence is handled by the game loop's periodic flush
//! system, which batches dirty chunks and sends them to the I/O thread
//! every ~30 seconds (configurable via `persistence_interval_seconds`).
//!
//! This plugin exists as a feature flag: when disabled (read-only or
//! lobby servers), the periodic flush is still active but `set_block`
//! is the only code that marks chunks dirty — and without the block
//! plugin, no mutations occur.
//!
//! The `PersistChunk` response is retained for explicit persistence
//! requests (e.g., graceful shutdown), but is no longer used for
//! per-mutation persistence.

use basalt_api::prelude::*;

/// Chunk persistence feature flag.
///
/// When registered, confirms that the server should persist block
/// changes to disk. The actual persistence is handled by the game
/// loop's batch flush system, not by per-event handlers.
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

    fn on_enable(&self, _registrar: &mut PluginRegistrar) {
        // Persistence is handled by the game loop's periodic dirty
        // chunk flush. No per-event handlers needed.
    }
}

#[cfg(test)]
mod tests {
    use basalt_testkit::PluginTestHarness;
    use basalt_types::Uuid;

    use super::*;

    #[test]
    fn block_changes_mark_chunks_dirty() {
        let mut harness = PluginTestHarness::new();
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

        // Block should be set (by BlockPlugin)
        assert_eq!(
            harness.world().get_block(5, 100, 3),
            basalt_world::block::STONE
        );
        // Chunk should be dirty (set_block marks it)
        let dirty = harness.world().dirty_chunks();
        assert!(
            dirty.contains(&(0, 0)),
            "chunk (0,0) should be dirty after block change"
        );
    }

    #[test]
    fn storage_with_memory_world_does_not_panic() {
        let mut harness = PluginTestHarness::new();
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
        assert_eq!(
            harness.world().get_block(5, 100, 3),
            basalt_world::block::STONE
        );
    }
}
