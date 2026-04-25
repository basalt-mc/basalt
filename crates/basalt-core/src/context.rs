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
    /// Returns the player's current world position. Captured at
    /// context-construction time — stale by the next tick.
    fn position(&self) -> (f64, f64, f64);
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
    /// Removes a block entity at the given position and fires a
    /// `BlockEntityDestroyedEvent` carrying the last state.
    ///
    /// No-op if no block entity exists at the position. Plugins use
    /// this from a `BlockBrokenEvent` Post handler to drive the
    /// destroy → drop-items chain through the event pipeline.
    fn destroy_block_entity(&self, x: i32, y: i32, z: i32);
}

/// Entity management: spawn, despawn, broadcast.
pub trait EntityContext {
    /// Spawns a dropped item entity at the given block coordinates.
    fn spawn_dropped_item(&self, x: i32, y: i32, z: i32, item_id: i32, count: i32);

    /// Broadcasts a block change to all connected players.
    fn broadcast_block_change(&self, x: i32, y: i32, z: i32, block_state: i32);

    /// Broadcasts an entity movement to all connected players.
    #[allow(clippy::too_many_arguments)]
    fn broadcast_entity_moved(
        &self,
        entity_id: i32,
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
        on_ground: bool,
    );

    /// Broadcasts that the current player has joined the server.
    ///
    /// The server constructs the broadcast payload (including skin data)
    /// from the context's player state.
    fn broadcast_player_joined(&self);

    /// Broadcasts that the current player has left the server.
    fn broadcast_player_left(&self);

    /// Sends a raw broadcast message to all connected players.
    ///
    /// Prefer the typed broadcast methods when possible. This method
    /// is for server-internal broadcasts that don't have typed wrappers.
    fn broadcast_raw(&self, msg: BroadcastMessage);

    /// Broadcasts a `BlockAction` packet to all connected players.
    ///
    /// Used for state-change animations driven by plugins (chest
    /// lid open/close, door swing, note-block pitch chime, etc.).
    /// The meaning of `action_id` and `action_param` is block-specific
    /// — see the wiki for the full table.
    fn broadcast_block_action(
        &self,
        x: i32,
        y: i32,
        z: i32,
        action_id: u8,
        action_param: u8,
        block_id: i32,
    );
}

/// Container interaction: chests, crafting tables, custom windows.
pub trait ContainerContext {
    /// Opens a chest container at the given position for the current player.
    fn open_chest(&self, x: i32, y: i32, z: i32);
    /// Opens a crafting table window at the given position for the current player.
    fn open_crafting_table(&self, x: i32, y: i32, z: i32);

    /// Opens a custom container window for the current player.
    ///
    /// Takes a reference so the `Container` can be stored in a static,
    /// cloned across calls, or shared — opening doesn't consume it.
    ///
    /// # Example
    /// ```ignore
    /// static SHOP: LazyLock<Container> = LazyLock::new(|| {
    ///     Container::builder()
    ///         .inventory_type(InventoryType::Generic9x6)
    ///         .title("Shop")
    ///         .build()
    /// });
    /// ctx.containers().open(&SHOP);
    /// ```
    fn open(&self, container: &crate::container::Container);

    /// Notifies every other player viewing the same block-backed
    /// container that a slot changed.
    ///
    /// Sends `SetContainerSlot` to all players whose `OpenContainer`
    /// component points at `(x, y, z)`, **excluding** the current
    /// player. Used by `ContainerPlugin` from the
    /// `ContainerSlotChangedEvent` handler to keep co-viewers in sync.
    /// No-op for virtual containers (they are per-player).
    fn notify_viewers(&self, x: i32, y: i32, z: i32, slot_index: i16, item: basalt_types::Slot);
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
