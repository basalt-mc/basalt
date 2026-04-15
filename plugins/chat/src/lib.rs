//! Chat broadcast plugin.
//!
//! Broadcasts chat messages to all connected players. Command
//! handling is in the separate `basalt-plugin-command` crate.

use basalt_api::prelude::*;
use basalt_types::{NamedColor, TextColor, TextComponent};

/// Broadcasts chat messages to all connected players.
///
/// - **Post ChatMessageEvent**: formats `<username> message` and broadcasts
pub struct ChatPlugin;

impl Plugin for ChatPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "chat",
            version: "0.1.0",
            author: Some("Basalt"),
            dependencies: &[],
        }
    }

    fn on_enable(&self, registrar: &mut PluginRegistrar) {
        registrar.on::<ChatMessageEvent>(Stage::Post, 0, |event, ctx| {
            let component = build_chat_component(&event.username, &event.message);
            ctx.broadcast_message_component(&component);
        });
    }
}

/// Builds a formatted chat text component for `<username> message`.
pub fn build_chat_component(username: &str, message: &str) -> TextComponent {
    TextComponent::text("<")
        .append(TextComponent::text(username).color(TextColor::Named(NamedColor::Aqua)))
        .append(TextComponent::text("> "))
        .append(TextComponent::text(message))
}

#[cfg(test)]
mod tests {
    use basalt_api::Response;
    use basalt_test_utils::PluginTestHarness;

    use super::*;

    #[test]
    fn chat_message_broadcasts() {
        let mut harness = PluginTestHarness::new();
        harness.register(ChatPlugin);

        let mut event = ChatMessageEvent {
            username: "Steve".into(),
            message: "hello".into(),
            cancelled: false,
        };

        let responses = harness.dispatch(&mut event);
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::Broadcast(BroadcastMessage::Chat { .. })
        ));
    }
}
