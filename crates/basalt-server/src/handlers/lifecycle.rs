//! Player lifecycle handler plugin.
//!
//! Handles player join and leave events: broadcasts notifications
//! to other connected players so they can update their tab list
//! and spawn/despawn entities.

use basalt_events::{EventBus, Stage};

use crate::context::{EventContext, Response};
use crate::events::{PlayerJoinedEvent, PlayerLeftEvent};
use crate::state::BroadcastMessage;

/// Handles player join and leave lifecycle events.
///
/// - **Post PlayerJoinedEvent**: broadcasts the join to all players
/// - **Post PlayerLeftEvent**: broadcasts the leave to all players
pub struct LifecycleHandler;

impl LifecycleHandler {
    /// Registers lifecycle handlers on the event bus.
    pub fn register(bus: &mut EventBus) {
        // Post: broadcast join to all players
        bus.on::<PlayerJoinedEvent, EventContext>(Stage::Post, 0, |event, ctx| {
            ctx.responses
                .push(Response::Broadcast(BroadcastMessage::PlayerJoined {
                    info: event.info.clone(),
                }));
        });

        // Post: broadcast leave to all players
        bus.on::<PlayerLeftEvent, EventContext>(Stage::Post, 0, |event, ctx| {
            ctx.responses
                .push(Response::Broadcast(BroadcastMessage::PlayerLeft {
                    uuid: event.uuid,
                    entity_id: event.entity_id,
                    username: event.username.clone(),
                }));
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use basalt_types::Uuid;

    use super::*;
    use crate::state::{PlayerSnapshot, ServerState};

    #[test]
    fn player_joined_broadcasts() {
        let state = ServerState::new();
        let ctx = EventContext::new(Arc::clone(&state));
        let mut event = PlayerJoinedEvent {
            info: PlayerSnapshot {
                username: "Steve".into(),
                uuid: Uuid::default(),
                entity_id: 1,
                x: 0.0,
                y: 64.0,
                z: 0.0,
                yaw: 0.0,
                pitch: 0.0,
                skin_properties: vec![],
            },
        };

        let mut bus = EventBus::new();
        LifecycleHandler::register(&mut bus);
        bus.dispatch(&mut event, &ctx);

        let responses = ctx.responses.drain();
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::Broadcast(BroadcastMessage::PlayerJoined { .. })
        ));
    }

    #[test]
    fn player_left_broadcasts() {
        let state = ServerState::new();
        let ctx = EventContext::new(Arc::clone(&state));
        let mut event = PlayerLeftEvent {
            uuid: Uuid::default(),
            entity_id: 1,
            username: "Steve".into(),
        };

        let mut bus = EventBus::new();
        LifecycleHandler::register(&mut bus);
        bus.dispatch(&mut event, &ctx);

        let responses = ctx.responses.drain();
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::Broadcast(BroadcastMessage::PlayerLeft { .. })
        ));
    }
}
