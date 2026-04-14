//! Event dispatch context and response queue.
//!
//! The [`ServerContext`] is the public API surface for event handlers.
//! It provides high-level methods for game actions (sending messages,
//! teleporting, broadcasting) and player identity. All methods queue
//! deferred responses that the server's play loop executes after
//! event dispatch completes.
//!
//! The [`Response`] enum and [`ResponseQueue`] are implementation
//! details — plugins interact only through `ServerContext` methods.

use std::cell::RefCell;

use basalt_types::nbt::NbtCompound;
use basalt_types::{TextComponent, Uuid};

use crate::broadcast::BroadcastMessage;

/// Context available to event handlers during dispatch.
///
/// Provides high-level methods for game actions and player identity.
/// Created per-dispatch on the stack. Handlers receive `&ServerContext`
/// (shared reference) and queue responses via the interior-mutable
/// response queue.
///
/// # Player identity
///
/// `player_uuid`, `player_entity_id`, and `player_username` identify
/// the player who triggered the event. These are copied from the
/// player's state at dispatch time.
pub struct ServerContext {
    /// Shared world reference for block access and chunk persistence.
    world: &'static basalt_world::World,
    /// Queue for deferred async responses.
    responses: ResponseQueue,
    /// UUID of the player who triggered this event.
    player_uuid: Uuid,
    /// Entity ID of the player who triggered this event.
    player_entity_id: i32,
    /// Username of the player who triggered this event.
    player_username: String,
    /// Name of the plugin currently being dispatched.
    plugin_name: RefCell<String>,
}

impl ServerContext {
    /// Creates a new context for a single event dispatch.
    ///
    /// Called internally by the play loop and connection handler.
    /// Not intended for plugin use — plugins receive `&ServerContext`.
    ///
    /// # Safety
    ///
    /// The `world` reference must outlive the context. In practice,
    /// the world lives in `Arc<ServerState>` which outlives all
    /// connection tasks.
    pub fn new(
        world: &'static basalt_world::World,
        player_uuid: Uuid,
        player_entity_id: i32,
        player_username: String,
    ) -> Self {
        Self {
            world,
            responses: ResponseQueue::new(),
            player_uuid,
            player_entity_id,
            player_username,
            plugin_name: RefCell::new(String::new()),
        }
    }

    /// Sets the plugin name for logger context.
    ///
    /// Called internally before each handler runs so `logger()`
    /// returns the correct target.
    pub fn set_plugin_name(&self, name: &str) {
        *self.plugin_name.borrow_mut() = name.to_string();
    }

    // --- Player identity ---

    /// Returns the UUID of the player who triggered this event.
    pub fn player_uuid(&self) -> Uuid {
        self.player_uuid
    }

    /// Returns the entity ID of the player who triggered this event.
    pub fn player_entity_id(&self) -> i32 {
        self.player_entity_id
    }

    /// Returns the username of the player who triggered this event.
    pub fn player_username(&self) -> &str {
        &self.player_username
    }

    // --- Logger ---

    /// Returns a logger scoped to the current plugin.
    ///
    /// Messages are logged with target `basalt::plugin::<name>`,
    /// making them easy to filter in log output.
    pub fn logger(&self) -> crate::logger::PluginLogger {
        crate::logger::PluginLogger::new(&self.plugin_name.borrow())
    }

    // --- World access ---

    /// Returns a reference to the world (chunks, blocks, persistence).
    ///
    /// Use `world().set_block()` to modify blocks, `world().get_block()`
    /// to read them, and `world().persist_chunk()` to save to disk.
    pub fn world(&self) -> &basalt_world::World {
        self.world
    }

    // --- Chat / messaging ---

    /// Sends a plain text chat message to the current player.
    pub fn send_message(&self, text: &str) {
        let component = TextComponent::text(text);
        self.send_message_component(&component);
    }

    /// Sends a styled chat message to the current player.
    ///
    /// Accepts a pre-built [`TextComponent`] for full control over
    /// colors, formatting, and text structure.
    pub fn send_message_component(&self, component: &TextComponent) {
        self.responses.push(Response::SendSystemChat {
            content: component.to_nbt(),
            action_bar: false,
        });
    }

    /// Sends an action bar message to the current player.
    ///
    /// Action bar messages appear above the hotbar and fade out.
    pub fn send_action_bar(&self, text: &str) {
        let component = TextComponent::text(text);
        self.responses.push(Response::SendSystemChat {
            content: component.to_nbt(),
            action_bar: true,
        });
    }

    /// Broadcasts a plain text chat message to ALL connected players.
    pub fn broadcast_message(&self, text: &str) {
        let component = TextComponent::text(text);
        self.broadcast_message_component(&component);
    }

    /// Broadcasts a styled chat message to ALL connected players.
    pub fn broadcast_message_component(&self, component: &TextComponent) {
        self.responses
            .push(Response::Broadcast(BroadcastMessage::Chat {
                content: component.to_nbt(),
            }));
    }

    // --- Player actions ---

    /// Teleports the current player to the given coordinates.
    pub fn teleport(&self, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) {
        self.responses.push(Response::SendPosition {
            teleport_id: 2,
            x,
            y,
            z,
            yaw,
            pitch,
        });
    }

    /// Changes the current player's gamemode.
    ///
    /// Mode values: 0 = survival, 1 = creative, 2 = adventure, 3 = spectator.
    pub fn set_gamemode(&self, mode: u8) {
        self.responses.push(Response::SendGameStateChange {
            reason: 3,
            value: mode as f32,
        });
    }

    // --- Block acknowledgement ---

    /// Sends a block action acknowledgement to the current player.
    ///
    /// The client waits for this before applying block predictions.
    /// The sequence number must match the client's dig/place packet.
    pub fn send_block_ack(&self, sequence: i32) {
        self.responses.push(Response::SendBlockAck { sequence });
    }

    // --- World streaming ---

    /// Streams chunks around the given chunk coordinates.
    ///
    /// Sends new chunks the player hasn't received yet and unloads
    /// chunks that are out of view distance.
    pub fn stream_chunks(&self, cx: i32, cz: i32) {
        self.responses.push(Response::StreamChunks {
            new_cx: cx,
            new_cz: cz,
        });
    }

    // --- Raw broadcast ---

    /// Sends a raw broadcast message to all connected players.
    ///
    /// Used for lifecycle events (join/leave), entity movement,
    /// and block changes. Prefer `broadcast_message()` for chat.
    pub fn broadcast(&self, msg: BroadcastMessage) {
        self.responses.push(Response::Broadcast(msg));
    }

    // --- Internal ---

    /// Drains all queued responses. Called by the play loop after dispatch.
    pub fn drain_responses(&self) -> Vec<Response> {
        self.responses.drain()
    }
}

/// Thread-local queue for deferred async responses.
///
/// Uses `RefCell` for interior mutability — handlers receive
/// `&ServerContext` (shared reference) but need to push responses.
/// This is safe because dispatch is single-threaded within a
/// connection task.
pub(crate) struct ResponseQueue {
    inner: RefCell<Vec<Response>>,
}

impl ResponseQueue {
    /// Creates an empty response queue.
    pub(crate) fn new() -> Self {
        Self {
            inner: RefCell::new(Vec::new()),
        }
    }

    /// Pushes a response onto the queue.
    pub(crate) fn push(&self, response: Response) {
        self.inner.borrow_mut().push(response);
    }

    /// Drains all queued responses, returning them as a Vec.
    pub(crate) fn drain(&self) -> Vec<Response> {
        self.inner.borrow_mut().drain(..).collect()
    }
}

/// A deferred async operation queued by a sync event handler.
///
/// After event dispatch completes, the play loop drains the response
/// queue and executes each response with access to the connection.
/// This enum is an implementation detail — plugins use `ServerContext`
/// methods instead.
#[derive(Debug, Clone)]
pub enum Response {
    /// Broadcast a message to all connected players.
    Broadcast(BroadcastMessage),
    /// Send a block action acknowledgement to the current player.
    SendBlockAck {
        /// Sequence number matching the client's dig/place packet.
        sequence: i32,
    },
    /// Send a system chat message to the current player.
    SendSystemChat {
        /// The formatted text component as NBT.
        content: NbtCompound,
        /// Whether to display as an action bar message.
        action_bar: bool,
    },
    /// Teleport the current player to a new position.
    SendPosition {
        /// Teleport ID for confirmation tracking.
        teleport_id: i32,
        /// Target X coordinate.
        x: f64,
        /// Target Y coordinate.
        y: f64,
        /// Target Z coordinate.
        z: f64,
        /// Target yaw angle.
        yaw: f32,
        /// Target pitch angle.
        pitch: f32,
    },
    /// Stream chunks around a new chunk position.
    StreamChunks {
        /// New chunk X coordinate.
        new_cx: i32,
        /// New chunk Z coordinate.
        new_cz: i32,
    },
    /// Send a game state change to the current player.
    SendGameStateChange {
        /// Reason code (e.g., 3 = change gamemode).
        reason: u8,
        /// Value associated with the reason.
        value: f32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_world() -> &'static basalt_world::World {
        use std::sync::OnceLock;
        static WORLD: OnceLock<basalt_world::World> = OnceLock::new();
        WORLD.get_or_init(|| basalt_world::World::new_memory(42))
    }

    fn test_ctx() -> ServerContext {
        ServerContext::new(test_world(), Uuid::default(), 1, "Steve".into())
    }

    #[test]
    fn player_identity() {
        let ctx = test_ctx();
        assert_eq!(ctx.player_uuid(), Uuid::default());
        assert_eq!(ctx.player_entity_id(), 1);
        assert_eq!(ctx.player_username(), "Steve");
    }

    #[test]
    fn world_access() {
        let ctx = test_ctx();
        // Should be able to read blocks
        let _block = ctx.world().get_block(0, 64, 0);
    }

    #[test]
    fn send_message_queues_response() {
        let ctx = test_ctx();
        ctx.send_message("hello");
        let responses = ctx.drain_responses();
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::SendSystemChat {
                action_bar: false,
                ..
            }
        ));
    }

    #[test]
    fn send_action_bar_queues_response() {
        let ctx = test_ctx();
        ctx.send_action_bar("bar");
        let responses = ctx.drain_responses();
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::SendSystemChat {
                action_bar: true,
                ..
            }
        ));
    }

    #[test]
    fn broadcast_message_queues_broadcast() {
        let ctx = test_ctx();
        ctx.broadcast_message("hello all");
        let responses = ctx.drain_responses();
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::Broadcast(BroadcastMessage::Chat { .. })
        ));
    }

    #[test]
    fn teleport_queues_position() {
        let ctx = test_ctx();
        ctx.teleport(10.0, 64.0, -5.0, 90.0, 0.0);
        let responses = ctx.drain_responses();
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::SendPosition { x, y, z, .. } if x == 10.0 && y == 64.0 && z == -5.0
        ));
    }

    #[test]
    fn set_gamemode_queues_state_change() {
        let ctx = test_ctx();
        ctx.set_gamemode(1);
        let responses = ctx.drain_responses();
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::SendGameStateChange { reason: 3, value } if value == 1.0
        ));
    }

    #[test]
    fn send_block_ack_queues_ack() {
        let ctx = test_ctx();
        ctx.send_block_ack(42);
        let responses = ctx.drain_responses();
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::SendBlockAck { sequence: 42 }
        ));
    }

    #[test]
    fn stream_chunks_queues_streaming() {
        let ctx = test_ctx();
        ctx.stream_chunks(1, 2);
        let responses = ctx.drain_responses();
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::StreamChunks {
                new_cx: 1,
                new_cz: 2
            }
        ));
    }

    #[test]
    fn drain_clears_queue() {
        let ctx = test_ctx();
        ctx.send_message("a");
        ctx.send_message("b");
        assert_eq!(ctx.drain_responses().len(), 2);
        assert!(ctx.drain_responses().is_empty());
    }
}
