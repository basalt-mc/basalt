//! Shared context trait for command handlers and plugins.
//!
//! The [`Context`] trait abstracts over the execution environment,
//! allowing commands and handlers to work with both in-game players
//! (`ServerContext`) and future console contexts.

use basalt_types::{TextComponent, Uuid};

use crate::broadcast::BroadcastMessage;
use crate::gamemode::Gamemode;

/// Execution context for commands and event handlers.
///
/// Provides identity information, messaging, player actions, and
/// world access. Implemented by `ServerContext` (in-game player)
/// and potentially `ConsoleContext` (server terminal) in the future.
pub trait Context {
    // --- Player identity ---

    /// Returns the UUID of the player who triggered this action.
    fn player_uuid(&self) -> Uuid;

    /// Returns the entity ID of the player.
    fn player_entity_id(&self) -> i32;

    /// Returns the username of the player.
    fn player_username(&self) -> &str;

    /// Returns the player's current yaw rotation (horizontal, degrees).
    fn player_yaw(&self) -> f32;

    /// Returns the player's current pitch rotation (vertical, degrees).
    fn player_pitch(&self) -> f32;

    // --- Logger ---

    /// Returns a logger scoped to the current plugin.
    fn logger(&self) -> crate::logger::PluginLogger;

    // --- World access ---

    /// Returns a reference to the world (chunks, blocks, persistence).
    fn world(&self) -> &basalt_world::World;

    // --- Chat / messaging ---

    /// Sends a plain text message to the current player.
    fn send_message(&self, text: &str);

    /// Sends a styled message to the current player.
    fn send_message_component(&self, component: &TextComponent);

    /// Sends an action bar message to the current player.
    fn send_action_bar(&self, text: &str);

    /// Broadcasts a plain text message to ALL connected players.
    fn broadcast_message(&self, text: &str);

    /// Broadcasts a styled message to ALL connected players.
    fn broadcast_message_component(&self, component: &TextComponent);

    // --- Player actions ---

    /// Teleports the current player to the given coordinates.
    fn teleport(&self, x: f64, y: f64, z: f64, yaw: f32, pitch: f32);

    /// Changes the current player's gamemode.
    fn set_gamemode(&self, mode: Gamemode);

    // --- Commands ---

    /// Returns a list of (name, description) for all registered commands.
    fn registered_commands(&self) -> Vec<(String, String)>;

    // --- Block ---

    /// Sends a block action acknowledgement to the current player.
    fn send_block_ack(&self, sequence: i32);

    // --- World streaming ---

    /// Streams chunks around the given chunk coordinates.
    fn stream_chunks(&self, cx: i32, cz: i32);

    // --- Persistence ---

    /// Schedules a chunk for asynchronous persistence on the I/O thread.
    ///
    /// The chunk is serialized and written to disk without blocking the
    /// game loop. This replaces direct `world().persist_chunk()` calls.
    fn persist_chunk(&self, cx: i32, cz: i32);

    // --- Raw broadcast ---

    /// Sends a raw broadcast message to all connected players.
    fn broadcast(&self, msg: BroadcastMessage);
}
