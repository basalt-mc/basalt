//! Chat broadcast plugin.
//!
//! Broadcasts chat messages to all connected players. Command
//! handling is in the separate `basalt-plugin-command` crate.

use basalt_api::prelude::*;
use basalt_api::types::{NamedColor, TextColor, TextComponent};

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
            let component = build_chat_component(ctx.player().username(), &event.message);
            ctx.chat().broadcast_component(&component);
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
    use basalt_testkit::PluginTestHarness;

    use super::*;

    #[test]
    fn chat_message_broadcasts() {
        let mut harness = PluginTestHarness::new();
        harness.register(ChatPlugin);

        let mut event = ChatMessageEvent {
            message: "hello".into(),
            cancelled: false,
        };

        let result = harness.dispatch(&mut event);
        assert_eq!(result.len(), 1);
        assert!(result.has_chat_broadcast());
    }

    #[test]
    fn cancelled_chat_produces_no_response() {
        let mut harness = PluginTestHarness::new();
        harness.on::<ChatMessageEvent>(Stage::Validate, 0, |event, _ctx| {
            event.cancel();
        });
        harness.register(ChatPlugin);

        let mut event = ChatMessageEvent {
            message: "spam".into(),
            cancelled: false,
        };

        let result = harness.dispatch(&mut event);
        assert!(result.is_empty());
    }

    #[test]
    fn empty_message_still_broadcasts() {
        let mut harness = PluginTestHarness::new();
        harness.register(ChatPlugin);

        let mut event = ChatMessageEvent {
            message: String::new(),
            cancelled: false,
        };

        let result = harness.dispatch(&mut event);
        assert_eq!(result.len(), 1);
    }
}
