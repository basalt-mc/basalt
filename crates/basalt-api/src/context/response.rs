//! Response enum and queue for deferred operations.

use std::cell::RefCell;

use basalt_core::broadcast::BroadcastMessage;
use basalt_core::components::{BlockPosition, ChunkPosition, Position, Rotation};
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
    /// Accepts a [`Container`](basalt_core::container::Container) template
    /// value built via [`Container::builder()`](basalt_core::container::Container::builder).
    OpenContainer(basalt_core::container::Container),
}

/// Thread-local queue for deferred async responses.
pub(crate) struct ResponseQueue {
    inner: RefCell<Vec<Response>>,
}

impl ResponseQueue {
    pub(crate) fn new() -> Self {
        Self {
            inner: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn push(&self, response: Response) {
        self.inner.borrow_mut().push(response);
    }

    pub(crate) fn drain(&self) -> Vec<Response> {
        self.inner.borrow_mut().drain(..).collect()
    }
}
