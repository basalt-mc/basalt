//! Player input handler plugin.
//!
//! Handles player movement: broadcasts position updates to other
//! connected players. Chunk streaming is handled by the
//! [`WorldHandler`](super::WorldHandler).

use basalt_events::{EventBus, Stage};

use crate::context::{EventContext, Response};
use crate::events::PlayerMovedEvent;
use crate::state::BroadcastMessage;

/// Handles player movement events.
///
/// - **Post**: broadcasts position/look updates to other players
pub struct PlayerInputHandler;

impl PlayerInputHandler {
    /// Registers movement handlers on the event bus.
    pub fn register(bus: &mut EventBus) {
        // Post: broadcast movement to other players
        bus.on::<PlayerMovedEvent, EventContext>(Stage::Post, 0, |event, ctx| {
            ctx.responses
                .push(Response::Broadcast(BroadcastMessage::EntityMoved {
                    entity_id: event.entity_id,
                    x: event.x,
                    y: event.y,
                    z: event.z,
                    yaw: event.yaw,
                    pitch: event.pitch,
                    on_ground: event.on_ground,
                }));
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::state::ServerState;

    #[test]
    fn movement_broadcasts() {
        let state = ServerState::new();
        let ctx = EventContext::new(Arc::clone(&state));
        let mut event = PlayerMovedEvent {
            entity_id: 1,
            x: 10.0,
            y: 64.0,
            z: 5.0,
            yaw: 90.0,
            pitch: 0.0,
            on_ground: true,
            old_cx: 0,
            old_cz: 0,
        };

        let mut bus = EventBus::new();
        PlayerInputHandler::register(&mut bus);
        bus.dispatch(&mut event, &ctx);

        let responses = ctx.responses.drain();
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::Broadcast(BroadcastMessage::EntityMoved { .. })
        ));
    }
}
