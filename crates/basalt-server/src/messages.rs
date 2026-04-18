//! Message types for communication between net tasks and the game loop.
//!
//! [`GameInput`]: net task -> game loop (game-relevant packets).
//! [`ServerOutput`]: game loop -> net task (game events to encode and send).
//!
//! The game loop expresses **game events**, not encoded packets. Net tasks
//! are responsible for translating events into protocol packets and encoding
//! them onto the wire.

use std::sync::Arc;

use basalt_core::broadcast::ProfileProperty;
use basalt_types::nbt::NbtCompound;
use basalt_types::{Encode, EncodedSize, Slot, Uuid};
use tokio::sync::mpsc;

/// Messages from net tasks to the game loop.
///
/// All net tasks share a single unbounded sender. The game loop
/// drains its receiver each tick via `try_recv()`.
pub enum GameInput {
    /// A new player has entered the Play state.
    ///
    /// The game loop spawns an ECS entity with all player components
    /// and sends the initial world data (Login, chunks, position).
    PlayerConnected {
        /// Server-assigned entity ID.
        entity_id: i32,
        /// Player UUID.
        uuid: Uuid,
        /// Player display name.
        username: String,
        /// Mojang skin texture data.
        skin_properties: Vec<ProfileProperty>,
        /// Initial spawn position.
        position: (f64, f64, f64),
        /// Initial yaw rotation.
        yaw: f32,
        /// Initial pitch rotation.
        pitch: f32,
        /// Channel for sending output packets to this player's net task.
        output_tx: mpsc::Sender<ServerOutput>,
    },
    /// A player has disconnected.
    PlayerDisconnected {
        /// UUID of the leaving player.
        uuid: Uuid,
    },
    /// Player position update.
    Position {
        /// UUID of the moving player.
        uuid: Uuid,
        /// New X coordinate.
        x: f64,
        /// New Y coordinate.
        y: f64,
        /// New Z coordinate.
        z: f64,
        /// Whether the player is on the ground.
        on_ground: bool,
    },
    /// Player look update.
    Look {
        /// UUID of the looking player.
        uuid: Uuid,
        /// New yaw angle (degrees).
        yaw: f32,
        /// New pitch angle (degrees).
        pitch: f32,
        /// Whether the player is on the ground.
        on_ground: bool,
    },
    /// Player position and look update.
    PositionLook {
        /// UUID of the moving player.
        uuid: Uuid,
        /// New X coordinate.
        x: f64,
        /// New Y coordinate.
        y: f64,
        /// New Z coordinate.
        z: f64,
        /// New yaw angle (degrees).
        yaw: f32,
        /// New pitch angle (degrees).
        pitch: f32,
        /// Whether the player is on the ground.
        on_ground: bool,
    },
    /// Block dig (status 0 = instant break in creative).
    BlockDig {
        /// UUID of the digging player.
        uuid: Uuid,
        /// Dig status.
        status: i32,
        /// Block X coordinate.
        x: i32,
        /// Block Y coordinate.
        y: i32,
        /// Block Z coordinate.
        z: i32,
        /// Sequence number for client acknowledgement.
        sequence: i32,
    },
    /// Block place.
    BlockPlace {
        /// UUID of the placing player.
        uuid: Uuid,
        /// Target block X coordinate.
        x: i32,
        /// Target block Y coordinate.
        y: i32,
        /// Target block Z coordinate.
        z: i32,
        /// Face direction (0-5).
        direction: i32,
        /// Sequence number for client acknowledgement.
        sequence: i32,
    },
    /// Player changed their held item slot.
    HeldItemSlot {
        /// UUID of the player.
        uuid: Uuid,
        /// New selected hotbar slot (0-8).
        slot: i16,
    },
    /// Player set a creative inventory slot.
    SetCreativeSlot {
        /// UUID of the player.
        uuid: Uuid,
        /// Inventory slot index (protocol slot).
        slot: i16,
        /// The item to place in the slot.
        item: Slot,
    },
    /// Player clicked in their inventory window.
    WindowClick {
        /// UUID of the player.
        uuid: Uuid,
        /// Protocol slot that was clicked (-999 for outside window).
        slot: i16,
        /// Mouse button (0 = left, 1 = right).
        button: i8,
        /// Click mode (0 = normal, 1 = shift, 4 = drop).
        mode: i32,
        /// Slots changed by the client.
        changed_slots: Vec<(i16, Slot)>,
        /// Item on the cursor after the click.
        cursor_item: Slot,
    },
    /// Player closed a container window.
    CloseWindow {
        /// UUID of the player.
        uuid: Uuid,
    },
    /// Player started or stopped sneaking.
    EntityAction {
        /// UUID of the player.
        uuid: Uuid,
        /// Action ID (0 = start sneak, 1 = stop sneak).
        action_id: i32,
    },
}

/// Output from the game loop to a player's net task.
///
/// Represents **game events**, not encoded packets. The net task matches
/// on each variant, constructs the appropriate protocol packet(s), and
/// encodes them onto the wire.
///
/// Variants are split by allocation cost:
/// - **Hot path**: small inline structs, zero heap allocation, cloned cheaply for broadcasts.
/// - **Chunk path**: coordinates only, net task looks up the shared [`ChunkPacketCache`].
/// - **Cold path**: rare events (connect/disconnect), one Arc alloc per event.
#[derive(Clone, Debug)]
pub enum ServerOutput {
    // ── Hot path (targeted, zero alloc) ─────────────────────────────────
    /// A block changed in the world. Net task sends BlockChange.
    BlockChanged {
        /// Block X coordinate.
        x: i32,
        /// Block Y coordinate.
        y: i32,
        /// Block Z coordinate.
        z: i32,
        /// New block state ID.
        state: i32,
    },
    /// Acknowledge a block dig/place sequence. Net task sends AcknowledgePlayerDigging.
    BlockAck {
        /// Sequence number to acknowledge.
        sequence: i32,
    },
    /// A system chat message. Net task sends SystemChat.
    SystemChat {
        /// NBT-encoded text component content.
        content: NbtCompound,
        /// Whether to display as action bar text.
        action_bar: bool,
    },
    /// A game state change. Net task sends GameStateChange.
    GameStateChange {
        /// Reason code (e.g., 13 = wait for chunks, 3 = change game mode).
        reason: u8,
        /// Associated float value (meaning depends on reason).
        value: f32,
    },
    /// Teleport or set player position. Net task sends Position.
    SetPosition {
        /// Teleport confirmation ID.
        teleport_id: i32,
        /// Target X coordinate.
        x: f64,
        /// Target Y coordinate.
        y: f64,
        /// Target Z coordinate.
        z: f64,
        /// Target yaw (degrees).
        yaw: f32,
        /// Target pitch (degrees).
        pitch: f32,
    },
    /// Update a single inventory slot on the client.
    SetSlot {
        /// Protocol slot index.
        slot: i16,
        /// The item in the slot.
        item: basalt_types::Slot,
    },
    /// Sync the full player inventory to the client.
    SyncInventory {
        /// All 46 protocol slots (crafting + armor + main + hotbar + offhand).
        slots: Vec<basalt_types::Slot>,
    },
    /// Open a container window on the client.
    OpenWindow {
        /// Window ID (1-127).
        window_id: u8,
        /// Inventory type (e.g., 2 = generic_9x3 for chests).
        inventory_type: i32,
        /// Window title (NBT text component).
        title: basalt_types::nbt::NbtCompound,
        /// Container slots + player inventory slots.
        slots: Vec<basalt_types::Slot>,
    },
    /// Update a slot in an open container window.
    SetContainerSlot {
        /// Window ID.
        window_id: u8,
        /// Slot index within the window.
        slot: i16,
        /// The item in the slot.
        item: basalt_types::Slot,
    },

    /// Inform client of a block entity at a position.
    /// Net task sends TileEntityData packet.
    BlockEntityData {
        /// Block position.
        x: i32,
        /// Block Y.
        y: i32,
        /// Block Z.
        z: i32,
        /// Block entity type (2 = chest).
        action: i32,
    },

    // ── Chunk path (cache-based, zero alloc) ──────────────────────────
    /// Send a chunk to the client. Net task looks up the ChunkPacketCache.
    SendChunk {
        /// Chunk X coordinate.
        cx: i32,
        /// Chunk Z coordinate.
        cz: i32,
    },
    /// Unload a chunk from the client.
    UnloadChunk {
        /// Chunk X coordinate.
        cx: i32,
        /// Chunk Z coordinate.
        cz: i32,
    },
    /// Start a chunk batch.
    ChunkBatchStart,
    /// Finish a chunk batch with the number of chunks sent.
    ChunkBatchFinished {
        /// Number of chunks in the batch.
        batch_size: i32,
    },
    /// Update the client's view position (center chunk).
    UpdateViewPosition {
        /// Center chunk X coordinate.
        cx: i32,
        /// Center chunk Z coordinate.
        cz: i32,
    },

    // ── Broadcast (shared, encoded once by first consumer) ──────────
    /// A broadcast game event shared across N players via `Arc`.
    ///
    /// The first net task to consume it encodes the protocol packets
    /// into the [`SharedBroadcast`]'s `OnceLock`. All subsequent net
    /// tasks read the cached bytes — one encode for N players.
    Broadcast(Arc<SharedBroadcast>),

    // ── Cold path (rare, Arc alloc OK) ────────────────────────────────
    /// A protocol packet to encode. Used for rare events (login, spawn)
    /// where a dedicated variant would add enum bloat.
    Packet(EncodablePacket),
    /// Pre-encoded packet bytes. Used for packets with manual encoding
    /// (PlayerInfo switch fields, DeclareCommands).
    Raw {
        /// Minecraft packet ID.
        id: i32,
        /// Encoded packet payload (without length prefix).
        data: Vec<u8>,
    },
}

/// A game event broadcast to multiple players.
///
/// Wraps a [`BroadcastEvent`] with lazy encoding: the first net task
/// to consume it encodes the protocol packets via [`OnceLock`], and
/// all subsequent consumers read the cached bytes.
pub struct SharedBroadcast {
    /// The game event to encode.
    pub(crate) event: BroadcastEvent,
    /// Cached encoded packets: `(packet_id, payload_bytes)`.
    /// Populated by the first net task that processes this broadcast.
    encoded: std::sync::OnceLock<Vec<(i32, Vec<u8>)>>,
}

impl SharedBroadcast {
    /// Creates a new shared broadcast from a game event.
    pub(crate) fn new(event: BroadcastEvent) -> Self {
        Self {
            event,
            encoded: std::sync::OnceLock::new(),
        }
    }

    /// Returns the cached encoded packets, encoding on first call.
    ///
    /// The `encode_fn` is called at most once (by the first consumer).
    /// All subsequent calls return the cached result.
    pub(crate) fn get_or_encode(
        &self,
        encode_fn: impl FnOnce(&BroadcastEvent) -> Vec<(i32, Vec<u8>)>,
    ) -> &[(i32, Vec<u8>)] {
        self.encoded.get_or_init(|| encode_fn(&self.event))
    }
}

impl std::fmt::Debug for SharedBroadcast {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedBroadcast")
            .field("event", &self.event)
            .field("encoded", &self.encoded.get().is_some())
            .finish()
    }
}

/// Game events that can be broadcast to multiple players.
///
/// Separate from [`ServerOutput`] to avoid recursive enum types.
/// These represent the subset of game events that are sent to N players
/// (movement, block changes, chat, player lifecycle).
#[derive(Clone, Debug)]
pub enum BroadcastEvent {
    /// An entity moved.
    EntityMoved {
        /// Entity ID.
        entity_id: i32,
        /// World X coordinate.
        x: f64,
        /// World Y coordinate.
        y: f64,
        /// World Z coordinate.
        z: f64,
        /// Yaw rotation (degrees).
        yaw: f32,
        /// Pitch rotation (degrees).
        pitch: f32,
        /// Whether the entity is on the ground.
        on_ground: bool,
    },
    /// A block changed in the world.
    BlockChanged {
        /// Block X coordinate.
        x: i32,
        /// Block Y coordinate.
        y: i32,
        /// Block Z coordinate.
        z: i32,
        /// New block state ID.
        state: i32,
    },
    /// A system chat message.
    SystemChat {
        /// NBT-encoded text component content.
        content: NbtCompound,
        /// Whether to display as action bar text.
        action_bar: bool,
    },
    /// Remove entities from the client.
    RemoveEntities {
        /// Entity IDs to remove.
        entity_ids: Vec<i32>,
    },
    /// Remove players from the tab list.
    RemovePlayers {
        /// Player UUIDs to remove.
        uuids: Vec<Uuid>,
    },
    /// A dropped item entity spawned in the world.
    ///
    /// Net task sends SpawnEntity (type 55) + SetEntityMetadata (index 8 = Slot).
    SpawnItemEntity {
        /// Entity ID.
        entity_id: i32,
        /// Spawn X coordinate.
        x: f64,
        /// Spawn Y coordinate.
        y: f64,
        /// Spawn Z coordinate.
        z: f64,
        /// Velocity X.
        vx: f64,
        /// Velocity Y.
        vy: f64,
        /// Velocity Z.
        vz: f64,
        /// Item ID.
        item_id: i32,
        /// Item count.
        count: i32,
    },
    /// A player picked up an item. Net task sends CollectItem.
    CollectItem {
        /// Entity ID of the collected item.
        collected_entity_id: i32,
        /// Entity ID of the player who picked it up.
        collector_entity_id: i32,
        /// Number of items picked up.
        count: i32,
    },
}

/// Supertrait combining [`Encode`] and [`EncodedSize`] for trait objects.
///
/// Rust only allows one non-auto trait in `dyn`, so this combines both
/// serialization traits into a single trait object-compatible trait.
pub(crate) trait PacketPayload: Encode + EncodedSize + Send + Sync {}
impl<T: Encode + EncodedSize + Send + Sync> PacketPayload for T {}

/// A type-erased protocol packet that can be encoded by the net task.
///
/// Wraps any packet struct implementing [`Encode`] + [`EncodedSize`]
/// behind an [`Arc`] for cheap cloning (needed for broadcast channel).
/// The Arc allocation only happens for cold-path packets (login, spawn).
#[derive(Clone)]
pub struct EncodablePacket {
    /// Minecraft packet ID.
    pub(crate) id: i32,
    /// The packet struct, type-erased for channel transport.
    pub(crate) payload: Arc<dyn PacketPayload>,
}

impl EncodablePacket {
    /// Creates a new encodable packet from any protocol packet struct.
    pub(crate) fn new<P: PacketPayload + 'static>(id: i32, packet: P) -> Self {
        Self {
            id,
            payload: Arc::new(packet),
        }
    }
}

impl std::fmt::Debug for EncodablePacket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncodablePacket")
            .field("id", &self.id)
            .field("size", &self.payload.encoded_size())
            .finish()
    }
}
