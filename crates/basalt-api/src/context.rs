//! Event dispatch context and response queue.
//!
//! The [`ServerContext`] is the concrete implementation of
//! [`Context`](basalt_core::Context) for in-game player contexts.
//! It queues deferred responses that the play loop executes after
//! event dispatch completes.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use basalt_core::broadcast::BroadcastMessage;
use basalt_core::gamemode::Gamemode;
use basalt_core::{Context, PluginLogger};
use basalt_types::nbt::NbtCompound;
use basalt_types::{TextComponent, Uuid};

/// Game state change reason codes from the Minecraft protocol.
///
/// Used in the GameStateChange packet (0x22) to indicate what kind of
/// state change is being communicated to the client.
const GAME_STATE_CHANGE_GAMEMODE: u8 = 3;

/// Context available to event handlers during dispatch.
///
/// Implements [`Context`] for in-game players. Created per-dispatch
/// on the stack. Internal methods (`new`, `set_plugin_name`,
/// `drain_responses`) are not part of the `Context` trait.
pub struct ServerContext {
    /// Shared world reference for block access and chunk persistence.
    world: Arc<basalt_world::World>,
    /// Queue for deferred async responses.
    responses: ResponseQueue,
    /// UUID of the player who triggered this event.
    player_uuid: Uuid,
    /// Entity ID of the player who triggered this event.
    player_entity_id: i32,
    /// Username of the player who triggered this event.
    player_username: String,
    /// Current yaw rotation of the player (horizontal, degrees).
    player_yaw: f32,
    /// Current pitch rotation of the player (vertical, degrees).
    player_pitch: f32,
    /// Name of the plugin currently being dispatched.
    plugin_name: RefCell<String>,
    /// Registered command list (name, description) for /help.
    command_list: RefCell<Vec<(String, String)>>,
    /// Monotonically increasing teleport ID counter shared across dispatches.
    teleport_counter: &'static AtomicI32,
}

/// Global teleport ID counter shared across all server contexts.
static GLOBAL_TELEPORT_COUNTER: AtomicI32 = AtomicI32::new(1);

impl ServerContext {
    /// Creates a new context for a single event dispatch.
    pub fn new(
        world: Arc<basalt_world::World>,
        player_uuid: Uuid,
        player_entity_id: i32,
        player_username: String,
        player_yaw: f32,
        player_pitch: f32,
    ) -> Self {
        Self {
            world,
            responses: ResponseQueue::new(),
            player_uuid,
            player_entity_id,
            player_username,
            player_yaw,
            player_pitch,
            plugin_name: RefCell::new(String::new()),
            command_list: RefCell::new(Vec::new()),
            teleport_counter: &GLOBAL_TELEPORT_COUNTER,
        }
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

/// Implementation of [`Context`] for in-game player contexts.
///
/// All methods queue [`Response`] variants into the interior-mutable
/// response queue. The play loop drains and executes them after
/// event dispatch completes.
impl Context for ServerContext {
    fn player_uuid(&self) -> Uuid {
        self.player_uuid
    }

    fn player_entity_id(&self) -> i32 {
        self.player_entity_id
    }

    fn player_username(&self) -> &str {
        &self.player_username
    }

    fn player_yaw(&self) -> f32 {
        self.player_yaw
    }

    fn player_pitch(&self) -> f32 {
        self.player_pitch
    }

    fn logger(&self) -> PluginLogger {
        PluginLogger::new(&self.plugin_name.borrow())
    }

    fn world(&self) -> &basalt_world::World {
        &self.world
    }

    fn send_message(&self, text: &str) {
        let component = TextComponent::text(text);
        self.send_message_component(&component);
    }

    fn send_message_component(&self, component: &TextComponent) {
        self.responses.push(Response::SendSystemChat {
            content: component.to_nbt(),
            action_bar: false,
        });
    }

    fn send_action_bar(&self, text: &str) {
        let component = TextComponent::text(text);
        self.responses.push(Response::SendSystemChat {
            content: component.to_nbt(),
            action_bar: true,
        });
    }

    fn broadcast_message(&self, text: &str) {
        let component = TextComponent::text(text);
        self.broadcast_message_component(&component);
    }

    fn broadcast_message_component(&self, component: &TextComponent) {
        self.responses
            .push(Response::Broadcast(BroadcastMessage::Chat {
                content: component.to_nbt(),
            }));
    }

    fn teleport(&self, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) {
        let teleport_id = self.teleport_counter.fetch_add(1, Ordering::Relaxed);
        self.responses.push(Response::SendPosition {
            teleport_id,
            x,
            y,
            z,
            yaw,
            pitch,
        });
    }

    fn set_gamemode(&self, mode: Gamemode) {
        self.responses.push(Response::SendGameStateChange {
            reason: GAME_STATE_CHANGE_GAMEMODE,
            value: mode.id() as f32,
        });
    }

    fn registered_commands(&self) -> Vec<(String, String)> {
        // Populated from ServerState command_args at dispatch time
        self.command_list.borrow().clone()
    }

    fn send_block_ack(&self, sequence: i32) {
        self.responses.push(Response::SendBlockAck { sequence });
    }

    fn stream_chunks(&self, cx: i32, cz: i32) {
        self.responses.push(Response::StreamChunks {
            new_cx: cx,
            new_cz: cz,
        });
    }

    fn persist_chunk(&self, cx: i32, cz: i32) {
        self.responses.push(Response::PersistChunk { cx, cz });
    }

    fn broadcast(&self, msg: BroadcastMessage) {
        self.responses.push(Response::Broadcast(msg));
    }
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

/// A deferred async operation queued by a sync event handler.
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
        /// Target coordinates and angles.
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
    },
    /// Stream chunks around a position.
    StreamChunks {
        /// Chunk coordinates.
        new_cx: i32,
        new_cz: i32,
    },
    /// Send a game state change.
    SendGameStateChange {
        /// Reason code.
        reason: u8,
        /// Associated value.
        value: f32,
    },
    /// Schedule a chunk for asynchronous persistence on the I/O thread.
    PersistChunk {
        /// Chunk X coordinate.
        cx: i32,
        /// Chunk Z coordinate.
        cz: i32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_world() -> Arc<basalt_world::World> {
        Arc::new(basalt_world::World::new_memory(42))
    }

    fn test_ctx() -> ServerContext {
        ServerContext::new(test_world(), Uuid::default(), 1, "Steve".into(), 0.0, 0.0)
    }

    #[test]
    fn player_identity() {
        let ctx = test_ctx();
        assert_eq!(ctx.player_uuid(), Uuid::default());
        assert_eq!(ctx.player_entity_id(), 1);
        assert_eq!(ctx.player_username(), "Steve");
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
    fn teleport_queues_position() {
        let ctx = test_ctx();
        ctx.teleport(10.0, 64.0, -5.0, 90.0, 0.0);
        let responses = ctx.drain_responses();
        assert_eq!(responses.len(), 1);
        assert!(matches!(responses[0], Response::SendPosition { .. }));
    }

    #[test]
    fn set_gamemode_queues_state_change() {
        let ctx = test_ctx();
        ctx.set_gamemode(Gamemode::Creative);
        let responses = ctx.drain_responses();
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0],
            Response::SendGameStateChange {
                reason: GAME_STATE_CHANGE_GAMEMODE,
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
    fn drain_clears_queue() {
        let ctx = test_ctx();
        ctx.send_message("a");
        ctx.send_message("b");
        assert_eq!(ctx.drain_responses().len(), 2);
        assert!(ctx.drain_responses().is_empty());
    }

    #[test]
    fn context_trait_is_usable_as_dyn() {
        let ctx = test_ctx();
        let dyn_ctx: &dyn Context = &ctx;
        dyn_ctx.send_message("via trait");
        assert_eq!(ctx.drain_responses().len(), 1);
    }
}
