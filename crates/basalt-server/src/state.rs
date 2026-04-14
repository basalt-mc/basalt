//! Shared server state for multi-player coordination.
//!
//! `ServerState` is the central shared state passed as `Arc<ServerState>`
//! to each connection task. It holds the player registry (who's online),
//! an atomic entity ID counter, and a broadcast channel for fan-out
//! messages to all connected players.

use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use basalt_api::broadcast::ProfileProperty;
pub(crate) use basalt_api::{BroadcastMessage, EventBus, PlayerSnapshot};
use basalt_types::Uuid;
use dashmap::DashMap;
use tokio::sync::broadcast;

/// Shared server state, held behind `Arc` and passed to every connection task.
pub(crate) struct ServerState {
    /// Atomic counter for assigning unique entity IDs.
    next_entity_id: AtomicI32,
    /// Lock-free registry of all connected players, keyed by UUID.
    players: DashMap<Uuid, PlayerHandle>,
    /// Broadcast channel sender — O(1) fan-out to all subscribers.
    broadcast_tx: broadcast::Sender<BroadcastMessage>,
    /// The world — chunk cache and terrain generator.
    pub world: basalt_world::World,
    /// Event bus with registered plugin handlers.
    pub event_bus: EventBus,
    /// Pre-built DeclareCommands packet payload (empty if no commands).
    pub declare_commands: Vec<u8>,
}

/// A handle to a connected player, stored in the server state registry.
///
/// Contains the player's identity info. Broadcast messages are delivered
/// via the shared `broadcast::Sender` rather than per-player channels.
#[derive(Debug)]
pub(crate) struct PlayerHandle {
    /// The player's display name.
    pub username: String,
    /// The player's UUID.
    pub uuid: Uuid,
    /// The player's unique entity ID.
    pub entity_id: i32,
    /// Mojang profile properties (skin textures).
    pub skin_properties: Vec<ProfileProperty>,
}

impl ServerState {
    /// Creates a new server state with default config (all plugins, read-write).
    #[cfg(test)]
    pub fn new() -> Arc<Self> {
        let config = crate::config::ServerConfig::default();
        Self::with_world_and_plugins(config.create_world(), config.create_plugins())
    }

    /// Creates a server state with a given world and plugin set.
    ///
    /// Each plugin's `on_enable` is called with a `PluginRegistrar`
    /// to register event handlers and commands. After all plugins
    /// are enabled, the collected commands are used to build the
    /// `DeclareCommands` packet and the command dispatch handler.
    pub fn with_world_and_plugins(
        world: basalt_world::World,
        plugins: Vec<Box<dyn basalt_api::Plugin>>,
    ) -> Arc<Self> {
        let (broadcast_tx, _) = broadcast::channel(256);
        let mut event_bus = EventBus::new();
        let mut commands = Vec::new();
        {
            let mut registrar = basalt_api::PluginRegistrar::new(&mut event_bus, &mut commands);
            for plugin in &plugins {
                log::info!(target: "basalt::plugin", "Enabling {} v{}", plugin.metadata().name, plugin.metadata().version);
                plugin.on_enable(&mut registrar);
            }
        }

        // Build DeclareCommands packet from all registered commands
        let declare_commands = build_declare_commands(&commands);

        // Register command dispatch handler on the event bus
        if !commands.is_empty() {
            let commands: Vec<basalt_api::CommandEntry> = commands.into_iter().collect();
            let commands = std::sync::Arc::new(commands);
            event_bus.on::<basalt_api::events::CommandEvent, basalt_api::context::ServerContext>(
                basalt_events::Stage::Process,
                -100, // high priority — runs before other Process handlers
                move |event, ctx| {
                    let parts: Vec<&str> = event.command.splitn(2, ' ').collect();
                    let cmd = parts[0];
                    let args = parts.get(1).copied().unwrap_or("");
                    let found = commands.iter().find(|c| c.name == cmd);
                    if let Some(entry) = found {
                        (entry.handler)(args, ctx);
                    } else {
                        ctx.send_message_component(
                            &basalt_types::TextComponent::text(format!("Unknown command: /{cmd}"))
                                .color(basalt_types::TextColor::Named(
                                    basalt_types::NamedColor::Red,
                                )),
                        );
                    }
                },
            );
        }

        log::info!(target: "basalt::server", "Registered {} commands", declare_commands.1);

        Arc::new(Self {
            next_entity_id: AtomicI32::new(1),
            players: DashMap::new(),
            broadcast_tx,
            world,
            declare_commands: declare_commands.0,
            event_bus,
        })
    }

    /// Allocates a unique entity ID for a new player.
    pub fn next_entity_id(&self) -> i32 {
        self.next_entity_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Subscribes to the broadcast channel. Each player task calls
    /// this once and polls the receiver in their play loop.
    pub fn subscribe(&self) -> broadcast::Receiver<BroadcastMessage> {
        self.broadcast_tx.subscribe()
    }

    /// Registers a player in the server state.
    ///
    /// Returns snapshots of all players who were already connected
    /// (the new player needs to know about them).
    pub fn register_player(&self, handle: PlayerHandle) -> Vec<PlayerSnapshot> {
        let existing: Vec<PlayerSnapshot> = self
            .players
            .iter()
            .map(|entry| {
                let h = entry.value();
                PlayerSnapshot {
                    username: h.username.clone(),
                    uuid: h.uuid,
                    entity_id: h.entity_id,
                    x: 0.0,
                    y: basalt_world::NoiseTerrainGenerator::SPAWN_Y as f64,
                    z: 0.0,
                    yaw: 0.0,
                    pitch: 0.0,
                    skin_properties: h.skin_properties.clone(),
                }
            })
            .collect();
        self.players.insert(handle.uuid, handle);
        existing
    }

    /// Removes a player from the server state.
    pub fn unregister_player(&self, uuid: &Uuid) {
        self.players.remove(uuid);
    }

    /// Broadcasts a message to all connected players.
    ///
    /// Uses the broadcast channel for O(1) fan-out. Receivers that
    /// have fallen behind will miss old messages (acceptable for
    /// movement updates, chat is rare enough to fit in the buffer).
    pub fn broadcast(&self, message: BroadcastMessage) {
        // Ignore send errors — they mean no receivers are listening.
        let _ = self.broadcast_tx.send(message);
    }

    /// Returns the number of currently connected players.
    #[cfg(test)]
    pub fn player_count(&self) -> usize {
        self.players.len()
    }
}

/// Builds the raw DeclareCommands packet payload from registered commands.
///
/// Returns (payload_bytes, command_count). The Brigadier tree has a root
/// node with one literal child per command, all marked as executable.
fn build_declare_commands(commands: &[basalt_api::CommandEntry]) -> (Vec<u8>, usize) {
    use basalt_types::{Encode, VarInt};

    let count = commands.len();
    if count == 0 {
        return (Vec::new(), 0);
    }

    let mut buf = Vec::new();

    // Total nodes = 1 root + N literals
    VarInt((count + 1) as i32).encode(&mut buf).unwrap();

    // Node 0: root (type=0, children=[1..=N])
    0u8.encode(&mut buf).unwrap(); // flags = root
    VarInt(count as i32).encode(&mut buf).unwrap();
    for i in 1..=count {
        VarInt(i as i32).encode(&mut buf).unwrap();
    }

    // Nodes 1..N: literal nodes, sorted by name
    let mut names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
    names.sort();
    for name in names {
        let flags: u8 = 0x01 | 0x04; // type=literal, executable
        flags.encode(&mut buf).unwrap();
        VarInt(0).encode(&mut buf).unwrap(); // no children
        // Minecraft string: VarInt length + UTF-8 bytes
        VarInt(name.len() as i32).encode(&mut buf).unwrap();
        buf.extend_from_slice(name.as_bytes());
    }

    // root_index = 0
    VarInt(0).encode(&mut buf).unwrap();

    (buf, count)
}

#[cfg(test)]
mod tests {
    use basalt_types::nbt::NbtCompound;

    use super::*;

    #[test]
    fn next_entity_id_increments() {
        let state = ServerState::new();
        assert_eq!(state.next_entity_id(), 1);
        assert_eq!(state.next_entity_id(), 2);
        assert_eq!(state.next_entity_id(), 3);
    }

    #[test]
    fn register_and_unregister_player() {
        let state = ServerState::new();

        let uuid = Uuid::default();
        let existing = state.register_player(PlayerHandle {
            username: "Steve".into(),
            uuid,
            entity_id: 1,
            skin_properties: vec![],
        });

        assert!(existing.is_empty());
        assert_eq!(state.player_count(), 1);

        state.unregister_player(&uuid);
        assert_eq!(state.player_count(), 0);
    }

    #[test]
    fn register_returns_existing_players() {
        let state = ServerState::new();

        let uuid1 = Uuid::from_bytes([1; 16]);
        let uuid2 = Uuid::from_bytes([2; 16]);

        state.register_player(PlayerHandle {
            username: "Alice".into(),
            uuid: uuid1,
            entity_id: 1,
            skin_properties: vec![],
        });

        let existing = state.register_player(PlayerHandle {
            username: "Bob".into(),
            uuid: uuid2,
            entity_id: 2,
            skin_properties: vec![],
        });

        assert_eq!(existing.len(), 1);
        assert_eq!(existing[0].username, "Alice");
        assert_eq!(state.player_count(), 2);
    }

    #[test]
    fn broadcast_delivers_to_subscriber() {
        let state = ServerState::new();
        let mut rx = state.subscribe();

        state.broadcast(BroadcastMessage::Chat {
            content: NbtCompound::new(),
        });

        let msg = rx.try_recv().unwrap();
        assert!(matches!(msg, BroadcastMessage::Chat { .. }));
    }

    #[test]
    fn broadcast_delivers_to_multiple_subscribers() {
        let state = ServerState::new();
        let mut rx1 = state.subscribe();
        let mut rx2 = state.subscribe();

        state.broadcast(BroadcastMessage::Chat {
            content: NbtCompound::new(),
        });

        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
    }
}
