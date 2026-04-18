//! Shared context trait for command handlers and plugins.
//!
//! The [`Context`] trait provides sub-contexts for different domains:
//! [`PlayerContext`], [`ChatContext`], [`WorldContext`], [`EntityContext`],
//! and [`ContainerContext`]. Plugins access them via `ctx.player()`,
//! `ctx.chat()`, etc.

use basalt_types::{TextComponent, Uuid};

use crate::broadcast::BroadcastMessage;
use crate::gamemode::Gamemode;

// ── Sub-context traits ───────────────────────────────────────────────

/// Player identity and state.
pub trait PlayerContext {
    /// Returns the UUID of the player who triggered this action.
    fn uuid(&self) -> Uuid;
    /// Returns the entity ID of the player.
    fn entity_id(&self) -> i32;
    /// Returns the username of the player.
    fn username(&self) -> &str;
    /// Returns the player's current yaw rotation (horizontal, degrees).
    fn yaw(&self) -> f32;
    /// Returns the player's current pitch rotation (vertical, degrees).
    fn pitch(&self) -> f32;
    /// Teleports the current player to the given coordinates.
    fn teleport(&self, x: f64, y: f64, z: f64, yaw: f32, pitch: f32);
    /// Changes the current player's gamemode.
    fn set_gamemode(&self, mode: Gamemode);
    /// Returns a list of (name, description) for all registered commands.
    fn registered_commands(&self) -> Vec<(String, String)>;
}

/// Chat and messaging.
pub trait ChatContext {
    /// Sends a plain text message to the current player.
    fn send(&self, text: &str);
    /// Sends a styled message to the current player.
    fn send_component(&self, component: &TextComponent);
    /// Sends an action bar message to the current player.
    fn action_bar(&self, text: &str);
    /// Broadcasts a plain text message to ALL connected players.
    fn broadcast(&self, text: &str);
    /// Broadcasts a styled message to ALL connected players.
    fn broadcast_component(&self, component: &TextComponent);
}

/// World access: blocks, chunks, persistence.
pub trait WorldContext {
    /// Returns a reference to the world (chunks, blocks, persistence).
    fn world(&self) -> &basalt_world::World;
    /// Sends a block action acknowledgement to the current player.
    fn send_block_ack(&self, sequence: i32);
    /// Streams chunks around the given chunk coordinates.
    fn stream_chunks(&self, cx: i32, cz: i32);
    /// Schedules a chunk for asynchronous persistence on the I/O thread.
    fn persist_chunk(&self, cx: i32, cz: i32);
}

/// Entity management: spawn, despawn, broadcast.
pub trait EntityContext {
    /// Spawns a dropped item entity at the given block coordinates.
    fn spawn_dropped_item(&self, x: i32, y: i32, z: i32, item_id: i32, count: i32);
    /// Sends a raw broadcast message to all connected players.
    fn broadcast_raw(&self, msg: BroadcastMessage);
}

/// Container interaction: chests, future crafting/furnaces.
pub trait ContainerContext {
    /// Opens a chest container at the given position for the current player.
    fn open_chest(&self, x: i32, y: i32, z: i32);
}

// ── Main Context trait ───────────────────────────────────────────────

/// Execution context for commands and event handlers.
///
/// Provides sub-context accessors for domain-specific operations.
/// Implemented by `ServerContext` (in-game player) and potentially
/// `ConsoleContext` (server terminal) in the future.
pub trait Context:
    PlayerContext + ChatContext + WorldContext + EntityContext + ContainerContext
{
    /// Returns a logger scoped to the current plugin.
    fn logger(&self) -> crate::logger::PluginLogger;

    /// Access player identity and state.
    fn player(&self) -> &dyn PlayerContext;

    /// Access chat and messaging.
    fn chat(&self) -> &dyn ChatContext;

    /// Access world, blocks, chunks, and persistence.
    fn world_ctx(&self) -> &dyn WorldContext;

    /// Access entity management.
    fn entities(&self) -> &dyn EntityContext;

    /// Access container interaction.
    fn containers(&self) -> &dyn ContainerContext;
}
