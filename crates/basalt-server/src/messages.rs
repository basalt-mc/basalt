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
    /// Player clicked a recipe in their book — the client expects
    /// a `CraftRecipeResponse` (ghost recipe) reply and the
    /// ingredients to be moved from the inventory into the crafting
    /// grid (auto-fill).
    PlaceRecipe {
        /// UUID of the requesting player.
        uuid: Uuid,
        /// Window id of the open crafting window.
        window_id: i32,
        /// Per-player numeric `display_id` of the chosen recipe.
        display_id: i32,
        /// Whether the player shift-clicked (asks for the largest
        /// possible batch). Currently degraded to a single craft.
        make_all: bool,
    },
    /// Client report of how many chunks per tick it can decode.
    ///
    /// Sent by the client after each chunk batch is fully received
    /// (`ServerboundPlayChunkBatchReceived`, packet `0x09`). The server
    /// uses the rate to throttle subsequent chunk sends so slow or
    /// distant clients are not flooded. Clamped on receipt to
    /// `[0.01, chunk_batch_max_rate]` to defend against malformed values.
    ChunkBatchAck {
        /// UUID of the reporting player.
        uuid: Uuid,
        /// Decoded chunks per tick measured by the client.
        chunks_per_tick: f32,
    },
}

/// Output from the game loop to a player's net task.
///
/// Sealed taxonomy of byte-production strategies, orthogonal to the
/// domain event that produced the send. The variant selects *how* the
/// payload bytes are materialised on the wire; the encoding work
/// itself happens at the construction site in `game/`, not in the
/// net task's dispatch.
#[derive(Clone, Debug)]
pub enum ServerOutput {
    /// Encode at write time. Single-packet payload carried by an
    /// [`EncodablePacket`] (Arc-wrapped for cheap cloning across the
    /// broadcast channel).
    Plain(EncodablePacket),

    /// Encode-once, share via `OnceLock` across N consumers.
    /// The first net task to drain it triggers the encode; every
    /// subsequent consumer reads the cached bytes — one encode per
    /// broadcast event regardless of the player count.
    Cached(Arc<SharedBroadcast>),

    /// Pre-encoded bytes valid for the entire process lifetime.
    /// Used for payloads computed once at boot (registry data, motd
    /// icon, etc.).
    #[allow(dead_code)]
    Static {
        /// Minecraft packet ID.
        id: i32,
        /// `'static` byte slice — typically pointing into a `LazyLock`
        /// or `OnceLock` initialised at boot.
        bytes: &'static [u8],
    },

    /// Pre-encoded reference-counted bytes. The `Arc<Vec<u8>>` lets
    /// multiple players consume the same encoded payload without
    /// cloning the underlying bytes — the chunk-packet cache and
    /// manually-encoded payloads (DeclareCommands, PlayerInfo) flow
    /// through this variant.
    RawBorrowed {
        /// Minecraft packet ID.
        id: i32,
        /// Reference-counted encoded payload.
        bytes: Arc<Vec<u8>>,
    },
}

impl ServerOutput {
    /// Convenience constructor for [`ServerOutput::Plain`] from a
    /// concrete protocol packet — wraps it in an [`EncodablePacket`]
    /// so call sites stay terse.
    pub(crate) fn plain<P: PacketPayload>(id: i32, packet: P) -> Self {
        Self::Plain(EncodablePacket::new(id, packet))
    }

    /// Convenience constructor for [`ServerOutput::RawBorrowed`] from
    /// an owned `Vec<u8>`. Used by call sites that build the bytes
    /// inline (manual encodings) and only need ref-counting for the
    /// channel hop.
    pub(crate) fn raw_owned(id: i32, data: Vec<u8>) -> Self {
        Self::RawBorrowed {
            id,
            bytes: Arc::new(data),
        }
    }
}

/// A multi-packet broadcast shared across N consumers via `Arc`.
///
/// Holds an ordered list of [`EncodablePacket`]s and a lazy
/// [`OnceLock`] cache of their encoded bytes. The first net task to
/// drain it triggers encoding; every subsequent consumer reads the
/// cached `(packet_id, payload_bytes)` pairs.
pub struct SharedBroadcast {
    /// The packets to send to each consumer, in order. Multi-packet
    /// broadcasts (e.g. spawn entity + metadata) are stored as separate
    /// entries; the encoder iterates through them.
    pub(crate) packets: Vec<EncodablePacket>,
    /// Cached encoded packets: `(packet_id, payload_bytes)`.
    /// Populated by the first net task that processes this broadcast.
    encoded: std::sync::OnceLock<Vec<(i32, Vec<u8>)>>,
}

impl SharedBroadcast {
    /// Creates a new shared broadcast from one or more packets.
    pub(crate) fn new(packets: Vec<EncodablePacket>) -> Self {
        Self {
            packets,
            encoded: std::sync::OnceLock::new(),
        }
    }

    /// Convenience constructor for single-packet broadcasts.
    pub(crate) fn single<P: PacketPayload>(id: i32, packet: P) -> Self {
        Self::new(vec![EncodablePacket::new(id, packet)])
    }

    /// Returns the cached encoded packets, encoding on first call.
    ///
    /// Encoding runs at most once (by the first consumer); all
    /// subsequent calls read the cached bytes — one encode per
    /// broadcast event regardless of the consumer count.
    pub(crate) fn get_or_encode(&self) -> &[(i32, Vec<u8>)] {
        self.encoded.get_or_init(|| {
            self.packets
                .iter()
                .map(|ep| {
                    let mut buf = Vec::with_capacity(ep.payload.encoded_size());
                    ep.payload
                        .encode(&mut buf)
                        .expect("packet encoding cannot fail");
                    (ep.id, buf)
                })
                .collect()
        })
    }
}

impl std::fmt::Debug for SharedBroadcast {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedBroadcast")
            .field("packets", &self.packets.len())
            .field("encoded", &self.encoded.get().is_some())
            .finish()
    }
}

/// Supertrait combining [`Encode`], [`EncodedSize`], and `Any`-style
/// downcasting for trait objects.
///
/// Rust only allows one non-auto trait in `dyn`, so this combines the
/// serialization traits into a single trait object-compatible trait.
/// `as_any` exposes the concrete payload type so tests can inspect
/// packet fields through [`EncodablePacket::downcast`].
pub(crate) trait PacketPayload: Encode + EncodedSize + Send + Sync + 'static {
    /// Returns a reference to the payload as `dyn Any` for downcasting.
    /// Used by [`EncodablePacket::downcast`] in tests.
    #[allow(dead_code)]
    fn as_any(&self) -> &dyn std::any::Any;
}
impl<T: Encode + EncodedSize + Send + Sync + 'static> PacketPayload for T {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

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
    pub(crate) fn new<P: PacketPayload>(id: i32, packet: P) -> Self {
        Self {
            id,
            payload: Arc::new(packet),
        }
    }

    /// Returns the wire packet ID this carrier holds.
    #[allow(dead_code)]
    pub fn id(&self) -> i32 {
        self.id
    }

    /// Attempts to downcast the type-erased payload to a concrete
    /// packet type. Returns `Some(&T)` if the underlying payload is
    /// exactly `T`, otherwise `None`.
    ///
    /// Used by tests to inspect packet fields. Production code only
    /// needs to encode and write the payload; downcasting is
    /// exclusively a test affordance.
    #[allow(dead_code)]
    pub fn downcast<T: PacketPayload>(&self) -> Option<&T> {
        self.payload.as_any().downcast_ref::<T>()
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
