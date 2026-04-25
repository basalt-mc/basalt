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
        registrar.on::<PlayerJoinedEvent>(Stage::Post, 0, |_event, ctx| {
            ctx.entities().broadcast_player_joined();
        });

        registrar.on::<PlayerLeftEvent>(Stage::Post, 0, |_event, ctx| {
            ctx.entities().broadcast_player_left();
        });
    }
}

#[cfg(test)]
mod tests {
    use basalt_api::testing::PluginTestHarness;

    use super::*;

    #[test]
    fn player_joined_broadcasts() {
        let mut harness = PluginTestHarness::new();
        harness.register(LifecyclePlugin);

        let mut event = PlayerJoinedEvent;

        let result = harness.dispatch(&mut event);
        assert_eq!(result.len(), 1);
        assert!(result.has_player_joined_broadcast());
    }

    #[test]
    fn player_left_broadcasts() {
        let mut harness = PluginTestHarness::new();
        harness.register(LifecyclePlugin);

        let mut event = PlayerLeftEvent;

        let result = harness.dispatch(&mut event);
        assert_eq!(result.len(), 1);
        assert!(result.has_player_left_broadcast());
    }
}
