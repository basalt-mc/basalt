//! Player lifecycle plugin.
//!
//! Broadcasts join/leave notifications to all connected players
//! so they can update their tab list and spawn/despawn entities.

use basalt_api::prelude::*;

/// Broadcasts player join and leave events.
///
/// - **Post PlayerJoinedEvent**: broadcasts join to all players
/// - **Post PlayerLeftEvent**: broadcasts leave to all players
pub struct LifecyclePlugin;

impl Plugin for LifecyclePlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "lifecycle",
            version: "0.1.0",
            author: Some("Basalt"),
            dependencies: &[],
        }
    }

    fn on_enable(&self, registrar: &mut PluginRegistrar) {
        registrar.on::<PlayerJoinedEvent>(Stage::Post, 0, |event, ctx| {
            ctx.entities()
                .broadcast_raw(BroadcastMessage::PlayerJoined {
                    info: event.info.clone(),
                });
        });

        registrar.on::<PlayerLeftEvent>(Stage::Post, 0, |event, ctx| {
            ctx.entities().broadcast_raw(BroadcastMessage::PlayerLeft {
                uuid: event.uuid,
                entity_id: event.entity_id,
                username: event.username.clone(),
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use basalt_api::Response;
    use basalt_testkit::PluginTestHarness;
    use basalt_types::Uuid;

    use super::*;

    #[test]
    fn player_joined_broadcasts() {
        let mut harness = PluginTestHarness::new();
        harness.register(LifecyclePlugin);

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

        let responses = harness.dispatch(&mut event);
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::Broadcast(BroadcastMessage::PlayerJoined { .. })
        ));
    }

    #[test]
    fn player_left_broadcasts() {
        let mut harness = PluginTestHarness::new();
        harness.register(LifecyclePlugin);

        let mut event = PlayerLeftEvent {
            uuid: Uuid::default(),
            entity_id: 1,
            username: "Steve".into(),
        };

        let responses = harness.dispatch(&mut event);
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::Broadcast(BroadcastMessage::PlayerLeft { .. })
        ));
    }
}
