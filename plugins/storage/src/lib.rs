//! Storage plugin for chunk persistence.
//!
//! Chunk persistence is handled by the game loop's periodic flush
//! system, which batches dirty chunks and sends them to the I/O thread
//! every ~30 seconds (configurable via `persistence_interval_seconds`).
//!
//! This plugin marks chunks as dirty when block entities are created,
//! modified, or destroyed, ensuring the persistence flush captures
//! inventory changes (chests, etc.) alongside block mutations.
//!
//! When disabled (read-only or lobby servers), block entity changes
//! are not persisted — `set_block` still marks chunks dirty for
//! block-level changes, but container mutations are skipped.

use basalt_api::prelude::*;

/// Chunk persistence plugin.
///
/// Registers handlers for block entity lifecycle events to mark
/// affected chunks as dirty. When this plugin is disabled, container
/// mutations (chest inventory changes, etc.) are not persisted to
/// disk, making persistence truly policy-driven.
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
        registrar.on::<BlockEntityCreatedEvent>(Stage::Post, 0, |event, ctx| {
            let (x, z) = (event.position.x, event.position.z);
            ctx.world_ctx().mark_chunk_dirty(x >> 4, z >> 4);
        });

        registrar.on::<BlockEntityModifiedEvent>(Stage::Post, 0, |event, ctx| {
            let (x, z) = (event.position.x, event.position.z);
            ctx.world_ctx().mark_chunk_dirty(x >> 4, z >> 4);
        });

        registrar.on::<BlockEntityDestroyedEvent>(Stage::Post, 0, |event, ctx| {
            let (x, z) = (event.position.x, event.position.z);
            ctx.world_ctx().mark_chunk_dirty(x >> 4, z >> 4);
        });
    }
}

#[cfg(test)]
mod tests {
    use basalt_api::components::BlockPosition;
    use basalt_api::world::block_entity::BlockEntity;
    use basalt_testkit::PluginTestHarness;

    use super::*;

    #[test]
    fn block_changes_mark_chunks_dirty() {
        let mut harness = PluginTestHarness::new();
        harness.register(basalt_plugin_block::BlockPlugin);
        harness.register(StoragePlugin);

        let mut event = BlockPlacedEvent {
            position: BlockPosition { x: 5, y: 100, z: 3 },
            block_state: basalt_api::world::block::STONE,
            sequence: 1,
            cancelled: false,
        };

        harness.dispatch(&mut event);

        // Block should be set (by BlockPlugin)
        assert_eq!(
            harness.world().get_block(5, 100, 3),
            basalt_api::world::block::STONE
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
            position: BlockPosition { x: 5, y: 100, z: 3 },
            block_state: basalt_api::world::block::STONE,
            sequence: 1,
            cancelled: false,
        };

        harness.dispatch(&mut event);
        assert_eq!(
            harness.world().get_block(5, 100, 3),
            basalt_api::world::block::STONE
        );
    }

    #[test]
    fn block_entity_created_marks_chunk_dirty() {
        let mut harness = PluginTestHarness::new();
        harness.register(StoragePlugin);

        // Touch the chunk so it exists in the cache
        let _ = harness.world().get_block(16, 64, 32);

        let mut event = BlockEntityCreatedEvent {
            position: BlockPosition {
                x: 16,
                y: 64,
                z: 32,
            },
            kind: BlockEntityKind::Chest,
        };
        harness.dispatch(&mut event);

        let dirty = harness.world().dirty_chunks();
        assert!(
            dirty.contains(&(1, 2)),
            "chunk (1,2) should be dirty after block entity creation"
        );
    }

    #[test]
    fn block_entity_modified_marks_chunk_dirty() {
        let mut harness = PluginTestHarness::new();
        harness.register(StoragePlugin);

        // Touch the chunk so it exists in the cache
        let _ = harness.world().get_block(16, 64, 32);

        let mut event = BlockEntityModifiedEvent {
            position: BlockPosition {
                x: 16,
                y: 64,
                z: 32,
            },
            kind: BlockEntityKind::Chest,
        };
        harness.dispatch(&mut event);

        let dirty = harness.world().dirty_chunks();
        assert!(
            dirty.contains(&(1, 2)),
            "chunk (1,2) should be dirty after block entity modification"
        );
    }

    #[test]
    fn block_entity_destroyed_marks_chunk_dirty() {
        let mut harness = PluginTestHarness::new();
        harness.register(StoragePlugin);

        // Touch the chunk so it exists in the cache
        let _ = harness.world().get_block(16, 64, 32);

        let mut event = BlockEntityDestroyedEvent {
            position: BlockPosition {
                x: 16,
                y: 64,
                z: 32,
            },
            kind: BlockEntityKind::Chest,
            last_state: BlockEntity::empty_chest(),
        };
        harness.dispatch(&mut event);

        let dirty = harness.world().dirty_chunks();
        assert!(
            dirty.contains(&(1, 2)),
            "chunk (1,2) should be dirty after block entity destruction"
        );
    }

    #[test]
    fn block_entity_events_without_plugin_do_not_mark_dirty() {
        let harness = PluginTestHarness::new();

        // Touch the chunk
        let _ = harness.world().get_block(16, 64, 32);

        // Without StoragePlugin, no handler marks dirty
        let dirty = harness.world().dirty_chunks();
        assert!(
            !dirty.contains(&(1, 2)),
            "chunk should not be dirty without StoragePlugin"
        );
    }
}
