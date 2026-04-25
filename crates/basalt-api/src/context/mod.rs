//! Event dispatch context, trait definitions, and response queue.
//!
//! The [`Context`] trait provides sub-contexts for different domains:
//! [`PlayerContext`], [`ChatContext`], [`WorldContext`], [`EntityContext`],
//! and [`ContainerContext`]. Plugins access them via `ctx.player()`,
//! `ctx.chat()`, etc.
//!
//! The [`ServerContext`] is the concrete implementation of [`Context`]
//! for in-game player contexts. It queues deferred responses that the
//! play loop executes after event dispatch completes.

mod chat;
mod container;
mod entity;
mod player;
mod recipe;
mod response;
mod world;

#[cfg(test)]
mod tests;

pub use response::Response;
pub(crate) use response::ResponseQueue;

use std::cell::RefCell;
use std::sync::Arc;

use basalt_recipes::RecipeId;
use basalt_types::{TextComponent, Uuid};

use crate::broadcast::BroadcastMessage;
use crate::components::KnownRecipes;
use crate::gamemode::Gamemode;
use crate::logger::PluginLogger;
use crate::player::PlayerInfo;

// ── Sub-context traits ───────────────────────────────────────────────

/// Why a recipe was unlocked for a player.
///
/// Surfaced in [`RecipeContext::unlock`] and on the
/// `RecipeUnlockedEvent` so plugins can branch on the source. For
/// example, an analytics plugin records `AutoDiscovered` differently
/// from `Manual` admin grants; a tutorial plugin only triggers on
/// `InitialJoin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnlockReason {
    /// The player crafted (or otherwise encountered) the recipe and
    /// the server auto-granted it.
    AutoDiscovered,
    /// A plugin or admin command granted the recipe.
    Manual,
    /// Granted as part of the initial recipe set when the player
    /// joined (starter recipes).
    InitialJoin,
}

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

/// Per-player recipe-book state.
///
/// Plugins use this to grant or revoke recipes for the current player
/// — the dispatch context's player. Mutations queue a deferred
/// response that the game loop translates into the appropriate S2C
/// recipe-book packet and dispatches `RecipeUnlockedEvent` /
/// `RecipeLockedEvent` after commit.
///
/// `has` and `unlocked` are synchronous reads against the player's
/// `KnownRecipes` component.
pub trait RecipeContext {
    /// Unlocks the recipe for the current player.
    ///
    /// Queues a `Recipe Book Add` packet, inserts into the player's
    /// `KnownRecipes`, and dispatches `RecipeUnlockedEvent` at Post.
    /// No-op if the recipe is already unlocked.
    fn unlock(&self, id: &RecipeId, reason: UnlockReason);

    /// Locks the recipe for the current player.
    ///
    /// Queues a `Recipe Book Remove` packet, removes from the player's
    /// `KnownRecipes`, and dispatches `RecipeLockedEvent` at Post.
    /// No-op if the recipe is not currently unlocked.
    fn lock(&self, id: &RecipeId);

    /// Returns true if the recipe is unlocked for the current player.
    fn has(&self, id: &RecipeId) -> bool;

    /// Returns a snapshot of every recipe id the player has unlocked.
    ///
    /// Allocates a new `Vec` — callers that only need to test for
    /// membership should prefer [`has`](Self::has).
    fn unlocked(&self) -> Vec<RecipeId>;
}

// ── Main Context trait ───────────────────────────────────────────────

/// Execution context for commands and event handlers.
///
/// Provides sub-context accessors for domain-specific operations.
/// Implemented by `ServerContext` (in-game player) and potentially
/// `ConsoleContext` (server terminal) in the future.
pub trait Context:
    PlayerContext + ChatContext + WorldContext + EntityContext + ContainerContext + RecipeContext
{
    /// Returns a logger scoped to the current plugin.
    fn logger(&self) -> PluginLogger;

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

    /// Access the current player's recipe-book state.
    fn recipes(&self) -> &dyn RecipeContext;
}

// ── ServerContext ────────────────────────────────────────────────────

/// Context available to event handlers during dispatch.
///
/// Implements [`Context`] for in-game players. Created per-dispatch
/// on the stack. Internal methods (`new`, `set_plugin_name`,
/// `drain_responses`) are not part of the `Context` trait.
pub struct ServerContext {
    /// Shared world reference for block access and chunk persistence.
    pub(super) world: Arc<basalt_world::World>,
    /// Queue for deferred async responses.
    pub(super) responses: ResponseQueue,
    /// Identity and state of the player who triggered this action.
    pub(super) player: PlayerInfo,
    /// Snapshot of the player's [`KnownRecipes`] at context construction
    /// — read-only view used by [`RecipeContext::has`] and
    /// [`RecipeContext::unlocked`]. Mutations queue a `Response::UnlockRecipe`
    /// / `LockRecipe` and only land in the ECS after dispatch completes.
    pub(super) known_recipes: KnownRecipes,
    /// Name of the plugin currently being dispatched.
    pub(super) plugin_name: RefCell<String>,
    /// Registered command list (name, description) for /help.
    pub(super) command_list: RefCell<Vec<(String, String)>>,
}

impl ServerContext {
    /// Creates a new context for a single event dispatch.
    ///
    /// The player's `KnownRecipes` snapshot defaults to empty —
    /// callers that need plugins to read live recipe state should use
    /// [`with_known_recipes`](Self::with_known_recipes) to attach a
    /// snapshot from the ECS.
    pub fn new(world: Arc<basalt_world::World>, player: PlayerInfo) -> Self {
        Self {
            world,
            responses: ResponseQueue::new(),
            player,
            known_recipes: KnownRecipes::default(),
            plugin_name: RefCell::new(String::new()),
            command_list: RefCell::new(Vec::new()),
        }
    }

    /// Attaches a snapshot of the player's current `KnownRecipes` so
    /// [`RecipeContext::has`] / [`RecipeContext::unlocked`] reflect
    /// live state. The server clones this from the ECS at the start
    /// of each dispatch.
    pub fn with_known_recipes(mut self, known_recipes: KnownRecipes) -> Self {
        self.known_recipes = known_recipes;
        self
    }

    /// Sets the registered command list for /help.
    pub fn set_command_list(&self, commands: Vec<(String, String)>) {
        *self.command_list.borrow_mut() = commands;
    }

    /// Sets the plugin name for logger context.
    pub fn set_plugin_name(&self, name: &str) {
        *self.plugin_name.borrow_mut() = name.to_string();
    }

    /// Drains all queued responses. Called by the play loop after dispatch.
    pub fn drain_responses(&self) -> Vec<Response> {
        self.responses.drain()
    }
}

impl Context for ServerContext {
    fn logger(&self) -> PluginLogger {
        PluginLogger::new(&self.plugin_name.borrow())
    }

    fn player(&self) -> &dyn PlayerContext {
        self
    }

    fn chat(&self) -> &dyn ChatContext {
        self
    }

    fn world_ctx(&self) -> &dyn WorldContext {
        self
    }

    fn entities(&self) -> &dyn EntityContext {
        self
    }

    fn containers(&self) -> &dyn ContainerContext {
        self
    }

    fn recipes(&self) -> &dyn RecipeContext {
        self
    }
}
