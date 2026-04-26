//! Response enum and queue for deferred operations.

use std::cell::RefCell;

use crate::broadcast::BroadcastMessage;
use crate::components::{BlockPosition, ChunkPosition, Position, Rotation};
use crate::context::UnlockReason;
use crate::recipes::RecipeId;
use basalt_types::Slot;
use basalt_types::nbt::NbtCompound;

/// A deferred operation queued by a sync event handler.
#[derive(Debug, Clone)]
pub enum Response {
    /// Broadcast a message to all connected players.
    Broadcast(BroadcastMessage),
    /// Send a block action acknowledgement.
    SendBlockAck {
        /// Sequence number.
        sequence: i32,
    },
    /// Send a system chat message.
    SendSystemChat {
        /// The formatted text component as NBT.
        content: NbtCompound,
        /// Whether to display as action bar.
        action_bar: bool,
    },
    /// Teleport the current player.
    SendPosition {
        /// Teleport ID.
        teleport_id: i32,
        /// Target position.
        position: Position,
        /// Target facing direction.
        rotation: Rotation,
    },
    /// Stream chunks around a chunk position.
    StreamChunks(ChunkPosition),
    /// Send a game state change.
    SendGameStateChange {
        /// Reason code.
        reason: u8,
        /// Associated value.
        value: f32,
    },
    /// Schedule a chunk for asynchronous persistence.
    PersistChunk(ChunkPosition),
    /// Spawn a dropped item entity in the world.
    SpawnDroppedItem {
        /// Block position where the item spawns.
        position: BlockPosition,
        /// Item ID to drop.
        item_id: i32,
        /// Item count.
        count: i32,
    },
    /// Open a chest container at the given position.
    OpenChest(BlockPosition),
    /// Open a crafting table window for the current player.
    ///
    /// Sends an OpenScreen packet with the 3x3 crafting grid type
    /// and attaches a CraftingGrid component to the player entity.
    OpenCraftingTable {
        /// Block position of the crafting table.
        position: BlockPosition,
    },
    /// Open a custom container window for the current player.
    ///
    /// Accepts a [`Container`](crate::container::Container) template
    /// value built via [`Container::builder()`](crate::container::Container::builder).
    OpenContainer(crate::container::Container),
    /// Broadcast a `BlockAction` packet to all connected players.
    ///
    /// Used by container/door/note-block plugins for state-change
    /// animations (chest lid open/close, door open/close, etc.). The
    /// meaning of `action_id` and `action_param` depends on the
    /// `block_id` registry value.
    BroadcastBlockAction {
        /// World position of the block.
        position: BlockPosition,
        /// Action identifier (block-specific).
        action_id: u8,
        /// Action parameter (block-specific; for chests this is the
        /// number of viewers, 0 = closed).
        action_param: u8,
        /// Block registry ID (e.g. 185 for chest in 1.21.4).
        block_id: i32,
    },
    /// Send a `SetContainerSlot` to every player viewing the same
    /// block-backed container, **excluding** the current player.
    ///
    /// Used by `ContainerPlugin` to keep co-viewers' open chests in
    /// sync when a slot is mutated by the source player. The server
    /// resolves which players are co-viewers by scanning the
    /// `OpenContainer` components.
    NotifyContainerViewers {
        /// World position of the block-backed container.
        position: BlockPosition,
        /// Protocol slot index that changed.
        slot_index: i16,
        /// New slot contents to broadcast.
        item: Slot,
    },
    /// Remove a block entity at the given position and dispatch
    /// `BlockEntityDestroyedEvent` with its last state.
    ///
    /// Used by `ContainerPlugin` on chest break to drive the destroy
    /// → drop-items chain through the event pipeline. No-op if no
    /// block entity exists at the position.
    DestroyBlockEntity {
        /// World position of the block entity to remove.
        position: BlockPosition,
    },
    /// Unlock a recipe for the current player.
    ///
    /// The server inserts the recipe id into the player's
    /// `KnownRecipes` component, sends a `Recipe Book Add` S2C packet,
    /// and dispatches `RecipeUnlockedEvent` at Post. No-op if the
    /// recipe is already unlocked.
    UnlockRecipe {
        /// Stable identifier of the recipe to unlock.
        recipe_id: RecipeId,
        /// Why the unlock happened — surfaced on `RecipeUnlockedEvent`.
        reason: UnlockReason,
    },
    /// Lock a recipe for the current player.
    ///
    /// The server removes the recipe from the player's `KnownRecipes`,
    /// sends a `Recipe Book Remove` S2C packet, and dispatches
    /// `RecipeLockedEvent` at Post. No-op if the recipe is not
    /// currently unlocked.
    LockRecipe {
        /// Stable identifier of the recipe to lock.
        recipe_id: RecipeId,
    },
}

/// Thread-local queue for deferred async responses.
///
/// Used by `ServerContext` implementations (basalt-server) to collect
/// deferred operations during handler dispatch. Visibility is `pub`
/// so that the production `ServerContext` in basalt-server can
/// construct and read from it.
pub struct ResponseQueue {
    inner: RefCell<Vec<Response>>,
}

impl Default for ResponseQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponseQueue {
    /// Creates an empty response queue.
    pub fn new() -> Self {
        Self {
            inner: RefCell::new(Vec::new()),
        }
    }

    /// Pushes a deferred response onto the queue.
    pub fn push(&self, response: Response) {
        self.inner.borrow_mut().push(response);
    }

    /// Drains all queued responses, returning them as a `Vec`.
    pub fn drain(&self) -> Vec<Response> {
        self.inner.borrow_mut().drain(..).collect()
    }
}
