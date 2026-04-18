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
use basalt_core::components::{BlockPosition, ChunkPosition, Position, Rotation};
use basalt_core::gamemode::Gamemode;
use basalt_core::player::PlayerInfo;
use basalt_core::{
    ChatContext, ContainerContext, Context, EntityContext, PlayerContext, PluginLogger,
    WorldContext,
};
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
    /// Identity and state of the player who triggered this action.
    player: PlayerInfo,
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
    pub fn new(world: Arc<basalt_world::World>, player: PlayerInfo) -> Self {
        Self {
            world,
            responses: ResponseQueue::new(),
            player,
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

impl PlayerContext for ServerContext {
    fn uuid(&self) -> Uuid {
        self.player.uuid
    }
    fn entity_id(&self) -> i32 {
        self.player.entity_id
    }
    fn username(&self) -> &str {
        &self.player.username
    }
    fn yaw(&self) -> f32 {
        self.player.rotation.yaw
    }
    fn pitch(&self) -> f32 {
        self.player.rotation.pitch
    }
    fn teleport(&self, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) {
        let teleport_id = self.teleport_counter.fetch_add(1, Ordering::Relaxed);
        self.responses.push(Response::SendPosition {
            teleport_id,
            position: Position { x, y, z },
            rotation: Rotation { yaw, pitch },
        });
    }
    fn set_gamemode(&self, mode: Gamemode) {
        self.responses.push(Response::SendGameStateChange {
            reason: GAME_STATE_CHANGE_GAMEMODE,
            value: mode.id() as f32,
        });
    }
    fn registered_commands(&self) -> Vec<(String, String)> {
        self.command_list.borrow().clone()
    }
}

impl ChatContext for ServerContext {
    fn send(&self, text: &str) {
        let component = TextComponent::text(text);
        self.send_component(&component);
    }
    fn send_component(&self, component: &TextComponent) {
        self.responses.push(Response::SendSystemChat {
            content: component.to_nbt(),
            action_bar: false,
        });
    }
    fn action_bar(&self, text: &str) {
        let component = TextComponent::text(text);
        self.responses.push(Response::SendSystemChat {
            content: component.to_nbt(),
            action_bar: true,
        });
    }
    fn broadcast(&self, text: &str) {
        let component = TextComponent::text(text);
        self.broadcast_component(&component);
    }
    fn broadcast_component(&self, component: &TextComponent) {
        self.responses
            .push(Response::Broadcast(BroadcastMessage::Chat {
                content: component.to_nbt(),
            }));
    }
}

impl WorldContext for ServerContext {
    fn world(&self) -> &basalt_world::World {
        &self.world
    }
    fn send_block_ack(&self, sequence: i32) {
        self.responses.push(Response::SendBlockAck { sequence });
    }
    fn stream_chunks(&self, cx: i32, cz: i32) {
        self.responses
            .push(Response::StreamChunks(ChunkPosition { x: cx, z: cz }));
    }
    fn persist_chunk(&self, cx: i32, cz: i32) {
        self.responses
            .push(Response::PersistChunk(ChunkPosition { x: cx, z: cz }));
    }
}

impl EntityContext for ServerContext {
    fn spawn_dropped_item(&self, x: i32, y: i32, z: i32, item_id: i32, count: i32) {
        self.responses.push(Response::SpawnDroppedItem {
            position: BlockPosition { x, y, z },
            item_id,
            count,
        });
    }
    fn broadcast_block_change(&self, x: i32, y: i32, z: i32, block_state: i32) {
        self.responses
            .push(Response::Broadcast(BroadcastMessage::BlockChanged {
                x,
                y,
                z,
                block_state,
            }));
    }
    fn broadcast_entity_moved(
        &self,
        entity_id: i32,
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
        on_ground: bool,
    ) {
        self.responses
            .push(Response::Broadcast(BroadcastMessage::EntityMoved {
                entity_id,
                x,
                y,
                z,
                yaw,
                pitch,
                on_ground,
            }));
    }
    fn broadcast_player_joined(&self) {
        self.responses
            .push(Response::Broadcast(BroadcastMessage::PlayerJoined {
                info: basalt_core::PlayerSnapshot {
                    username: self.player.username.clone(),
                    uuid: self.player.uuid,
                    entity_id: self.player.entity_id,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    yaw: self.player.rotation.yaw,
                    pitch: self.player.rotation.pitch,
                    skin_properties: Vec::new(),
                },
            }));
    }
    fn broadcast_player_left(&self) {
        self.responses
            .push(Response::Broadcast(BroadcastMessage::PlayerLeft {
                uuid: self.player.uuid,
                entity_id: self.player.entity_id,
                username: self.player.username.clone(),
            }));
    }
    fn broadcast_raw(&self, msg: BroadcastMessage) {
        self.responses.push(Response::Broadcast(msg));
    }
}

impl ContainerContext for ServerContext {
    fn open_chest(&self, x: i32, y: i32, z: i32) {
        self.responses
            .push(Response::OpenChest(BlockPosition { x, y, z }));
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_world() -> Arc<basalt_world::World> {
        Arc::new(basalt_world::World::new_memory(42))
    }

    fn test_ctx() -> ServerContext {
        ServerContext::new(
            test_world(),
            PlayerInfo {
                uuid: Uuid::default(),
                entity_id: 1,
                username: "Steve".into(),
                rotation: Rotation {
                    yaw: 0.0,
                    pitch: 0.0,
                },
            },
        )
    }

    #[test]
    fn player_identity() {
        let ctx = test_ctx();
        assert_eq!(ctx.player().uuid(), Uuid::default());
        assert_eq!(ctx.player().entity_id(), 1);
        assert_eq!(ctx.player().username(), "Steve");
    }

    #[test]
    fn send_message_queues_response() {
        let ctx = test_ctx();
        ctx.chat().send("hello");
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
        ctx.player().teleport(10.0, 64.0, -5.0, 90.0, 0.0);
        let responses = ctx.drain_responses();
        assert_eq!(responses.len(), 1);
        assert!(matches!(responses[0], Response::SendPosition { .. }));
    }

    #[test]
    fn set_gamemode_queues_state_change() {
        let ctx = test_ctx();
        ctx.player().set_gamemode(Gamemode::Creative);
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
        ctx.chat().broadcast("hello all");
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
        ctx.chat().send("a");
        ctx.chat().send("b");
        assert_eq!(ctx.drain_responses().len(), 2);
        assert!(ctx.drain_responses().is_empty());
    }

    #[test]
    fn context_trait_is_usable_as_dyn() {
        let ctx = test_ctx();
        let dyn_ctx: &dyn Context = &ctx;
        dyn_ctx.chat().send("via trait");
        assert_eq!(ctx.drain_responses().len(), 1);
    }
}
