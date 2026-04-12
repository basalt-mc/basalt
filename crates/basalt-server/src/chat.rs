//! Chat formatting helpers.
//!
//! Provides text component builders for chat messages and system
//! messages. Command dispatch has been moved to the `ChatHandler`
//! plugin in `handlers/chat.rs`.

use basalt_net::connection::{Connection, Play};
use basalt_protocol::packets::play::chat::ClientboundPlaySystemChat;
use basalt_types::{NamedColor, TextColor, TextComponent};

/// Sends a system chat message to the player.
///
/// Builds a `SystemChat` packet from a `TextComponent` and writes it
/// to the connection. Use `is_action_bar: true` to show the message
/// in the action bar instead of the chat window.
pub(crate) async fn send_system_message(
    conn: &mut Connection<Play>,
    component: &TextComponent,
    is_action_bar: bool,
) -> crate::error::Result<()> {
    let packet = ClientboundPlaySystemChat {
        content: component.to_nbt(),
        is_action_bar,
    };
    conn.write_packet_typed(ClientboundPlaySystemChat::PACKET_ID, &packet)
        .await?;
    Ok(())
}

/// Sends a welcome message when a player joins the server.
pub(crate) async fn send_welcome(
    conn: &mut Connection<Play>,
    username: &str,
) -> crate::error::Result<()> {
    let msg = TextComponent::text(format!("Welcome to Basalt, {username}!"))
        .color(TextColor::Named(NamedColor::Gold))
        .bold(true);
    send_system_message(conn, &msg, false).await
}

/// Builds a formatted chat text component for `<username> message`.
///
/// Returns the `TextComponent` without sending it — the caller
/// broadcasts it to all players via `ServerState::broadcast`.
pub(crate) fn build_chat_component(username: &str, message: &str) -> TextComponent {
    TextComponent::text("<")
        .append(TextComponent::text(username).color(TextColor::Named(NamedColor::Aqua)))
        .append(TextComponent::text("> "))
        .append(TextComponent::text(message))
}
