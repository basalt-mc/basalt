//! World handler plugin.
//!
//! Handles world-related concerns: chunk streaming when a player
//! crosses a chunk boundary. When this plugin is disabled (e.g.,
//! auth server), no chunks are sent and players exist in a void.

use basalt_events::{EventBus, Stage};

use crate::context::{EventContext, Response};
use crate::events::PlayerMovedEvent;

/// Handles world-related events: chunk streaming on movement.
///
/// - **Process PlayerMovedEvent**: detects chunk boundary crossings
///   and queues chunk streaming
///
/// Disabling this handler means players never receive chunk data
/// after the initial world load — useful for auth or lobby servers.
pub struct WorldHandler;

impl WorldHandler {
    /// Registers world handlers on the event bus.
    pub fn register(bus: &mut EventBus) {
        // Process: check for chunk boundary crossing and queue streaming
        bus.on::<PlayerMovedEvent, EventContext>(Stage::Process, 0, |event, ctx| {
            let new_cx = (event.x as i32) >> 4;
            let new_cz = (event.z as i32) >> 4;
            if new_cx != event.old_cx || new_cz != event.old_cz {
                ctx.responses
                    .push(Response::StreamChunks { new_cx, new_cz });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::state::ServerState;

    #[test]
    fn chunk_boundary_crossing_queues_stream() {
        let state = ServerState::new();
        let ctx = EventContext::new(Arc::clone(&state));
        let mut event = PlayerMovedEvent {
            entity_id: 1,
            x: 16.0, // chunk 1
            y: 64.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: true,
            old_cx: 0, // was in chunk 0
            old_cz: 0,
        };

        let mut bus = EventBus::new();
        WorldHandler::register(&mut bus);
        bus.dispatch(&mut event, &ctx);

        let responses = ctx.responses.drain();
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
    fn same_chunk_no_streaming() {
        let state = ServerState::new();
        let ctx = EventContext::new(Arc::clone(&state));
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

        let mut bus = EventBus::new();
        WorldHandler::register(&mut bus);
        bus.dispatch(&mut event, &ctx);

        assert!(ctx.responses.drain().is_empty());
    }
}
