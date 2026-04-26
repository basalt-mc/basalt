//! ChatContext implementation for ServerContext.

use basalt_api::broadcast::BroadcastMessage;
use basalt_api::context::{ChatContext, Response};
use basalt_types::TextComponent;

use super::ServerContext;

impl ChatContext for ServerContext {
    fn send(&self, text: &str) {
        let component = TextComponent::text(text);
        self.send_component(&component);
    }
    fn send_component(&self, component: &TextComponent) {
        self.responses.push(Response::SendSystemChat {
            content: component.to_nbt(),
            action_bar: false,
        });
    }
    fn action_bar(&self, text: &str) {
        let component = TextComponent::text(text);
        self.responses.push(Response::SendSystemChat {
            content: component.to_nbt(),
            action_bar: true,
        });
    }
    fn broadcast(&self, text: &str) {
        let component = TextComponent::text(text);
        self.broadcast_component(&component);
    }
    fn broadcast_component(&self, component: &TextComponent) {
        self.responses
            .push(Response::Broadcast(BroadcastMessage::Chat {
                content: component.to_nbt(),
            }));
    }
}
