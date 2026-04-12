//! Chat message handling and command dispatch.
//!
//! Processes incoming chat messages and slash commands from players.
//! Chat messages are echoed back as `SystemChat` packets. Commands
//! are parsed and executed, with feedback sent to the player.

use basalt_net::connection::{Connection, Play};
use basalt_protocol::packets::play::chat::ClientboundPlaySystemChat;
use basalt_protocol::packets::play::player::{
    ClientboundPlayGameStateChange, ClientboundPlayPosition,
};
use basalt_types::{NamedColor, TextColor, TextComponent};

use crate::player::PlayerState;

/// Sends a system chat message to the player.
///
/// Builds a `SystemChat` packet from a `TextComponent` and writes it
/// to the connection. Use `is_action_bar: true` to show the message
/// in the action bar instead of the chat window.
pub(crate) async fn send_system_message(
    conn: &mut Connection<Play>,
    component: &TextComponent,
    is_action_bar: bool,
) -> basalt_net::Result<()> {
    let packet = ClientboundPlaySystemChat {
        content: component.to_nbt(),
        is_action_bar,
    };
    conn.write_packet_typed(ClientboundPlaySystemChat::PACKET_ID, &packet)
        .await
}

/// Sends a welcome message when a player joins the server.
pub(crate) async fn send_welcome(
    conn: &mut Connection<Play>,
    username: &str,
) -> basalt_net::Result<()> {
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

/// Handles a slash command from the player.
///
/// Parses the command string and dispatches to the appropriate handler.
/// Unknown commands receive a red error message.
pub(crate) async fn handle_command(
    conn: &mut Connection<Play>,
    player: &mut PlayerState,
    command: &str,
) -> basalt_net::Result<()> {
    let parts: Vec<&str> = command.splitn(2, ' ').collect();
    let cmd = parts[0];
    let args = parts.get(1).unwrap_or(&"");

    match cmd {
        "say" => cmd_say(conn, args).await,
        "tp" => cmd_tp(conn, player, args).await,
        "gamemode" => cmd_gamemode(conn, args).await,
        "help" => cmd_help(conn).await,
        _ => {
            let msg = TextComponent::text(format!("Unknown command: /{cmd}"))
                .color(TextColor::Named(NamedColor::Red));
            send_system_message(conn, &msg, false).await
        }
    }
}

/// `/say <message>` — broadcasts a server message.
async fn cmd_say(conn: &mut Connection<Play>, message: &str) -> basalt_net::Result<()> {
    let msg = TextComponent::text("[Server] ")
        .color(TextColor::Named(NamedColor::LightPurple))
        .bold(true)
        .append(TextComponent::text(message).color(TextColor::Named(NamedColor::White)));
    send_system_message(conn, &msg, false).await
}

/// `/tp <x> <y> <z>` — teleports the player to the given coordinates.
async fn cmd_tp(
    conn: &mut Connection<Play>,
    player: &mut PlayerState,
    args: &str,
) -> basalt_net::Result<()> {
    let coords: Vec<&str> = args.split_whitespace().collect();
    if coords.len() != 3 {
        let msg =
            TextComponent::text("Usage: /tp <x> <y> <z>").color(TextColor::Named(NamedColor::Red));
        return send_system_message(conn, &msg, false).await;
    }

    let x: f64 = match coords[0].parse() {
        Ok(v) => v,
        Err(_) => {
            let msg = TextComponent::text("Invalid x coordinate")
                .color(TextColor::Named(NamedColor::Red));
            return send_system_message(conn, &msg, false).await;
        }
    };
    let y: f64 = match coords[1].parse() {
        Ok(v) => v,
        Err(_) => {
            let msg = TextComponent::text("Invalid y coordinate")
                .color(TextColor::Named(NamedColor::Red));
            return send_system_message(conn, &msg, false).await;
        }
    };
    let z: f64 = match coords[2].parse() {
        Ok(v) => v,
        Err(_) => {
            let msg = TextComponent::text("Invalid z coordinate")
                .color(TextColor::Named(NamedColor::Red));
            return send_system_message(conn, &msg, false).await;
        }
    };

    player.update_position(x, y, z);
    player.teleport_confirmed = false;

    let position = ClientboundPlayPosition {
        teleport_id: 2,
        x,
        y,
        z,
        dx: 0.0,
        dy: 0.0,
        dz: 0.0,
        yaw: player.yaw,
        pitch: player.pitch,
        flags: 0,
    };
    conn.write_packet_typed(ClientboundPlayPosition::PACKET_ID, &position)
        .await?;

    let msg = TextComponent::text(format!("Teleported to {x}, {y}, {z}"))
        .color(TextColor::Named(NamedColor::Green));
    send_system_message(conn, &msg, false).await
}

/// `/gamemode <mode>` — changes the player's gamemode.
///
/// Accepted modes: survival (0), creative (1), adventure (2), spectator (3).
async fn cmd_gamemode(conn: &mut Connection<Play>, args: &str) -> basalt_net::Result<()> {
    let mode: f32 = match args.trim() {
        "survival" | "0" => 0.0,
        "creative" | "1" => 1.0,
        "adventure" | "2" => 2.0,
        "spectator" | "3" => 3.0,
        _ => {
            let msg =
                TextComponent::text("Usage: /gamemode <survival|creative|adventure|spectator>")
                    .color(TextColor::Named(NamedColor::Red));
            return send_system_message(conn, &msg, false).await;
        }
    };

    // GameEvent reason=3 = change game mode
    let event = ClientboundPlayGameStateChange {
        reason: 3,
        game_mode: mode,
    };
    conn.write_packet_typed(ClientboundPlayGameStateChange::PACKET_ID, &event)
        .await?;

    let name = match mode as u8 {
        0 => "Survival",
        1 => "Creative",
        2 => "Adventure",
        _ => "Spectator",
    };
    let msg = TextComponent::text(format!("Game mode set to {name}"))
        .color(TextColor::Named(NamedColor::Green));
    send_system_message(conn, &msg, false).await
}

/// `/help` — shows available commands.
async fn cmd_help(conn: &mut Connection<Play>) -> basalt_net::Result<()> {
    let msg = TextComponent::text("Available commands:")
        .color(TextColor::Named(NamedColor::Gold))
        .append(
            TextComponent::text("\n /say <message>").color(TextColor::Named(NamedColor::Yellow)),
        )
        .append(
            TextComponent::text(" — broadcast a server message")
                .color(TextColor::Named(NamedColor::Gray)),
        )
        .append(
            TextComponent::text("\n /tp <x> <y> <z>").color(TextColor::Named(NamedColor::Yellow)),
        )
        .append(
            TextComponent::text(" — teleport to coordinates")
                .color(TextColor::Named(NamedColor::Gray)),
        )
        .append(
            TextComponent::text("\n /gamemode <mode>").color(TextColor::Named(NamedColor::Yellow)),
        )
        .append(
            TextComponent::text(" — change game mode").color(TextColor::Named(NamedColor::Gray)),
        )
        .append(TextComponent::text("\n /help").color(TextColor::Named(NamedColor::Yellow)))
        .append(TextComponent::text(" — show this help").color(TextColor::Named(NamedColor::Gray)));
    send_system_message(conn, &msg, false).await
}
