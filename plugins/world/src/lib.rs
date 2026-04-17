//! World chunk streaming plugin.
//!
//! Streams chunks to players when they cross chunk boundaries.
//! Without this plugin, players only see the initial chunks sent
//! at login — no new terrain loads as they move.

use basalt_api::prelude::*;

/// Streams chunks on player chunk boundary crossings.
///
/// - **Process PlayerMovedEvent**: detects boundary crossing, queues chunk streaming
pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "world",
            version: "0.1.0",
            author: Some("Basalt"),
            dependencies: &[],
        }
    }

    fn on_enable(&self, registrar: &mut PluginRegistrar) {
        registrar.on::<PlayerMovedEvent>(Stage::Process, 0, |event, ctx| {
            let new_cx = (event.x.floor() as i32) >> 4;
            let new_cz = (event.z.floor() as i32) >> 4;
            if new_cx != event.old_cx || new_cz != event.old_cz {
                ctx.stream_chunks(new_cx, new_cz);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use basalt_api::Response;
    use basalt_testkit::PluginTestHarness;

    use super::*;

    #[test]
    fn chunk_boundary_crossing_queues_stream() {
        let mut harness = PluginTestHarness::new();
        harness.register(WorldPlugin);

        let mut event = PlayerMovedEvent {
            entity_id: 1,
            x: 16.0,
            y: 64.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: true,
            old_cx: 0,
            old_cz: 0,
        };

        let responses = harness.dispatch(&mut event);
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::StreamChunks {
                new_cx: 1,
                new_cz: 0
            }
        ));
    }

    #[test]
    fn negative_coordinate_chunk_boundary() {
        let mut harness = PluginTestHarness::new();
        harness.register(WorldPlugin);

        // x=-0.5 is in chunk -1, not chunk 0 (floor before shift)
        let mut event = PlayerMovedEvent {
            entity_id: 1,
            x: -0.5,
            y: 64.0,
            z: -0.5,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: true,
            old_cx: 0,
            old_cz: 0,
        };

        let responses = harness.dispatch(&mut event);
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::StreamChunks {
                new_cx: -1,
                new_cz: -1
            }
        ));
    }

    #[test]
    fn same_chunk_no_streaming() {
        let mut harness = PluginTestHarness::new();
        harness.register(WorldPlugin);

        let mut event = PlayerMovedEvent {
            entity_id: 1,
            x: 5.0,
            y: 64.0,
            z: 5.0,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: true,
            old_cx: 0,
            old_cz: 0,
        };

        assert!(harness.dispatch(&mut event).is_empty());
    }
}
