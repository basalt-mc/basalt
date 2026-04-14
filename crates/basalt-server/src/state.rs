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
use basalt_core::Context;
use basalt_types::Uuid;
use dashmap::DashMap;
use tokio::sync::broadcast;

/// Shared server state, held behind `Arc` and passed to every connection task.
pub(crate) struct ServerState {
    /// Atomic counter for assigning unique entity IDs.
    next_entity_id: AtomicI32,
    /// Lock-free registry of all connected players, keyed by UUID.
    pub(crate) players: DashMap<Uuid, PlayerHandle>,
    /// Broadcast channel sender — O(1) fan-out to all subscribers.
    broadcast_tx: broadcast::Sender<BroadcastMessage>,
    /// The world — chunk cache and terrain generator.
    pub world: basalt_world::World,
    /// Event bus with registered plugin handlers.
    pub event_bus: EventBus,
    /// Pre-built DeclareCommands packet payload (empty if no commands).
    pub declare_commands: Vec<u8>,
    /// Command arg metadata for TabComplete suggestions.
    pub command_args: Vec<CommandMeta>,
}

/// Command metadata for TabComplete suggestions.
///
/// Stores only the argument schema — not the handler.
pub(crate) struct CommandMeta {
    /// Command name.
    pub name: String,
    /// Command description.
    pub description: String,
    /// Argument schemas (empty if no variants).
    pub args: Vec<basalt_command::CommandArg>,
    /// Variant argument schemas.
    pub variants: Vec<Vec<basalt_command::CommandArg>>,
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

        // Extract arg metadata before moving commands
        let command_args: Vec<CommandMeta> = commands
            .iter()
            .map(|c| CommandMeta {
                name: c.name.clone(),
                description: c.description.clone(),
                args: c.args.clone(),
                variants: c.variants.clone(),
            })
            .collect();

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
                        match basalt_command::parse_command_args(args, &entry.args, &entry.variants)
                        {
                            Ok(parsed) => (entry.handler)(&parsed, ctx),
                            Err(msg) => {
                                let err_msg = format!("/{cmd}: {msg}");
                                ctx.send_message_component(
                                    &basalt_types::TextComponent::text(err_msg).color(
                                        basalt_types::TextColor::Named(
                                            basalt_types::NamedColor::Red,
                                        ),
                                    ),
                                );
                            }
                        }
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
            command_args,
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
/// Generates a Brigadier command tree with argument nodes for
/// tab-completion. Commands with variants get multiple child branches.
/// A node in the Brigadier command tree being constructed.
struct BrigNode {
    flags: u8,
    children: Vec<i32>,
    name: Option<String>,
    parser: Option<Vec<u8>>,
    suggestions: Option<String>,
}

/// Encodes parser ID and properties for a Brigadier argument node.
/// IDs from 1.21.4 protocol.json parser registry.
fn encode_parser(arg_type: &basalt_command::Arg) -> Vec<u8> {
    use basalt_command::Arg;
    use basalt_types::{Encode, VarInt};

    let mut data = Vec::new();
    match arg_type {
        Arg::Boolean => {
            VarInt(0).encode(&mut data).unwrap();
        }
        Arg::Double => {
            VarInt(2).encode(&mut data).unwrap();
            0u8.encode(&mut data).unwrap();
        }
        Arg::Integer => {
            VarInt(3).encode(&mut data).unwrap();
            0u8.encode(&mut data).unwrap();
        }
        Arg::String | Arg::Options(_) => {
            VarInt(5).encode(&mut data).unwrap();
            VarInt(0).encode(&mut data).unwrap(); // SINGLE_WORD
        }
        Arg::Entity => {
            VarInt(6).encode(&mut data).unwrap();
            0x00u8.encode(&mut data).unwrap(); // multiple entities, any type
        }
        Arg::Player => {
            VarInt(6).encode(&mut data).unwrap();
            0x01u8.encode(&mut data).unwrap(); // single entity
        }
        Arg::GameProfile => {
            VarInt(7).encode(&mut data).unwrap();
        }
        Arg::BlockPos => {
            VarInt(8).encode(&mut data).unwrap();
        }
        Arg::ColumnPos => {
            VarInt(9).encode(&mut data).unwrap();
        }
        Arg::Vec3 => {
            VarInt(10).encode(&mut data).unwrap();
        }
        Arg::Vec2 => {
            VarInt(11).encode(&mut data).unwrap();
        }
        Arg::BlockState => {
            VarInt(12).encode(&mut data).unwrap();
        }
        Arg::ItemStack => {
            VarInt(14).encode(&mut data).unwrap();
        }
        Arg::Component => {
            VarInt(17).encode(&mut data).unwrap();
        }
        Arg::Message => {
            VarInt(19).encode(&mut data).unwrap();
        }
        Arg::Rotation => {
            VarInt(28).encode(&mut data).unwrap();
        }
        Arg::ResourceLocation => {
            VarInt(35).encode(&mut data).unwrap();
        }
        Arg::Uuid => {
            VarInt(53).encode(&mut data).unwrap();
        }
    };
    data
}

/// Recursively builds argument nodes from a set of arg chains,
/// merging shared prefixes into single nodes with multiple children.
///
/// Each chain is a slice of `CommandArg`. Chains sharing the same
/// first argument (name + type) are grouped under one node.
fn build_arg_trie(
    chains: &[&[basalt_command::CommandArg]],
    nodes: &mut Vec<BrigNode>,
    parent_children: &mut Vec<i32>,
) {
    // Group chains by first arg name, preserving insertion order
    let mut group_order: Vec<String> = Vec::new();
    let mut group_map: std::collections::HashMap<String, Vec<&[basalt_command::CommandArg]>> =
        std::collections::HashMap::new();

    for chain in chains {
        if chain.is_empty() {
            continue;
        }
        let key = chain[0].name.clone();
        if !group_map.contains_key(&key) {
            group_order.push(key.clone());
        }
        group_map.entry(key).or_default().push(chain);
    }

    for key in &group_order {
        let group = &group_map[key];
        let first_arg = &group[0][0];
        // Disable ask_server suggestions for now — test if tree
        // parses correctly without it
        let has_suggestions = false;

        // A variant ends here if its chain is length 1
        let is_executable = group.iter().any(|c| c.len() == 1);

        let mut flags: u8 = 0x02; // argument
        if is_executable {
            flags |= 0x04;
        }
        if has_suggestions {
            flags |= 0x10;
        }

        let node_idx = nodes.len();
        parent_children.push(node_idx as i32);

        nodes.push(BrigNode {
            flags,
            children: Vec::new(),
            name: Some(first_arg.name.clone()),
            parser: Some(encode_parser(&first_arg.arg_type)),
            suggestions: if has_suggestions {
                Some("minecraft:ask_server".to_string())
            } else {
                None
            },
        });

        // Recurse with remaining args (skip first element of each chain)
        let sub_chains: Vec<&[basalt_command::CommandArg]> = group
            .iter()
            .filter(|c| c.len() > 1)
            .map(|c| &c[1..])
            .collect();

        if !sub_chains.is_empty() {
            let mut children = Vec::new();
            build_arg_trie(&sub_chains, nodes, &mut children);
            nodes[node_idx].children = children;
        }
    }
}

fn build_declare_commands(commands: &[basalt_api::CommandEntry]) -> (Vec<u8>, usize) {
    use basalt_types::{Encode, VarInt};

    let count = commands.len();
    if count == 0 {
        return (Vec::new(), 0);
    }

    let mut sorted: Vec<&basalt_api::CommandEntry> = commands.iter().collect();
    sorted.sort_by_key(|c| &c.name);

    let mut nodes: Vec<BrigNode> = Vec::new();

    // Node 0: root
    nodes.push(BrigNode {
        flags: 0x00,
        children: Vec::new(),
        name: None,
        parser: None,
        suggestions: None,
    });

    // Reserve literal nodes (indices 1..=count)
    let literal_start = 1;
    for _ in &sorted {
        nodes.push(BrigNode {
            flags: 0x01,
            children: Vec::new(),
            name: None,
            parser: None,
            suggestions: None,
        });
    }

    nodes[0].children = (literal_start..literal_start + count)
        .map(|i| i as i32)
        .collect();

    for (cmd_i, cmd) in sorted.iter().enumerate() {
        let literal_idx = literal_start + cmd_i;
        nodes[literal_idx].name = Some(cmd.name.clone());

        let arg_lists: Vec<&Vec<basalt_command::CommandArg>> = if !cmd.variants.is_empty() {
            cmd.variants.iter().collect()
        } else if !cmd.args.is_empty() {
            vec![&cmd.args]
        } else {
            nodes[literal_idx].flags |= 0x04; // executable, no args
            continue;
        };

        // Convert to slices for the trie builder
        let chains: Vec<&[basalt_command::CommandArg]> =
            arg_lists.iter().map(|v| v.as_slice()).collect();

        let mut children = Vec::new();
        build_arg_trie(&chains, &mut nodes, &mut children);
        nodes[literal_idx].children = children;
    }

    // Serialize
    let mut buf = Vec::new();
    VarInt(nodes.len() as i32).encode(&mut buf).unwrap();

    for node in &nodes {
        node.flags.encode(&mut buf).unwrap();
        VarInt(node.children.len() as i32).encode(&mut buf).unwrap();
        for &child in &node.children {
            VarInt(child).encode(&mut buf).unwrap();
        }
        if let Some(name) = &node.name {
            encode_string(name, &mut buf);
        }
        if let Some(parser) = &node.parser {
            buf.extend_from_slice(parser);
        }
        if let Some(suggestions) = &node.suggestions {
            encode_string(suggestions, &mut buf);
        }
    }

    VarInt(0).encode(&mut buf).unwrap(); // root_index
    (buf, count)
}

/// Encodes a Minecraft protocol string (VarInt length + UTF-8 bytes).
fn encode_string(s: &str, buf: &mut Vec<u8>) {
    use basalt_types::{Encode, VarInt};
    VarInt(s.len() as i32).encode(buf).unwrap();
    buf.extend_from_slice(s.as_bytes());
}

#[cfg(test)]
mod tests {
    use basalt_types::Decode;
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

    #[test]
    fn declare_commands_tp_has_three_variants() {
        use basalt_command::{Arg, CommandArg, Validation};

        // Build /tp with 4 variants matching vanilla:
        //   /tp <destination>              (entity 0x01)
        //   /tp <location>                 (vec3)
        //   /tp <targets> <destination>    (entity 0x00 → entity 0x01)
        //   /tp <targets> <location>       (entity 0x00 → vec3)
        // The trie merges the two <targets> variants into one node.
        let commands = vec![basalt_api::CommandEntry {
            name: "tp".into(),
            description: "Teleport".into(),
            args: Vec::new(),
            variants: vec![
                vec![CommandArg {
                    name: "destination".into(),
                    arg_type: Arg::Player,
                    validation: Validation::Auto,
                    required: true,
                }],
                vec![CommandArg {
                    name: "location".into(),
                    arg_type: Arg::Vec3,
                    validation: Validation::Auto,
                    required: true,
                }],
                vec![
                    CommandArg {
                        name: "targets".into(),
                        arg_type: Arg::Entity,
                        validation: Validation::Auto,
                        required: true,
                    },
                    CommandArg {
                        name: "destination".into(),
                        arg_type: Arg::Player,
                        validation: Validation::Auto,
                        required: true,
                    },
                ],
                vec![
                    CommandArg {
                        name: "targets".into(),
                        arg_type: Arg::Entity,
                        validation: Validation::Auto,
                        required: true,
                    },
                    CommandArg {
                        name: "location".into(),
                        arg_type: Arg::Vec3,
                        validation: Validation::Auto,
                        required: true,
                    },
                ],
            ],
            handler: Box::new(|_args, _ctx| {}),
        }];

        let (payload, count) = build_declare_commands(&commands);
        assert_eq!(count, 1);

        let mut cursor: &[u8] = &payload;
        let node_count = basalt_types::VarInt::decode(&mut cursor).unwrap().0;
        // root + literal "tp" + destination + location + targets + targets/destination + targets/location = 7
        assert_eq!(node_count, 7, "expected 7 nodes in tp tree");

        // Node 0: root
        let flags = u8::decode(&mut cursor).unwrap();
        assert_eq!(flags, 0x00);
        let child_count = basalt_types::VarInt::decode(&mut cursor).unwrap().0;
        assert_eq!(child_count, 1);
        let _tp_idx = basalt_types::VarInt::decode(&mut cursor).unwrap().0;

        // Node 1: literal "tp" — 3 children (destination, location, targets)
        let flags = u8::decode(&mut cursor).unwrap();
        assert_eq!(flags & 0x03, 0x01); // literal
        assert_eq!(flags & 0x04, 0x00); // NOT executable
        let child_count = basalt_types::VarInt::decode(&mut cursor).unwrap().0;
        assert_eq!(
            child_count, 3,
            "tp should have 3 children: destination, location, targets"
        );
        // Read child indices
        let dest_idx = basalt_types::VarInt::decode(&mut cursor).unwrap().0;
        let _loc_idx = basalt_types::VarInt::decode(&mut cursor).unwrap().0;
        let _tgt_idx = basalt_types::VarInt::decode(&mut cursor).unwrap().0;
        // Read name "tp"
        let name_len = basalt_types::VarInt::decode(&mut cursor).unwrap().0 as usize;
        let mut name_buf = vec![0u8; name_len];
        name_buf.copy_from_slice(&cursor[..name_len]);
        cursor = &cursor[name_len..];
        assert_eq!(std::str::from_utf8(&name_buf).unwrap(), "tp");

        // Node 2: destination (entity parser 6, flags 0x01)
        let flags = u8::decode(&mut cursor).unwrap();
        assert_eq!(flags & 0x03, 0x02, "destination should be argument type");
        assert_ne!(flags & 0x04, 0, "destination should be executable");
        assert_eq!(dest_idx, 2);

        // Verify the rest has content (don't decode everything)
        assert!(
            cursor.len() > 10,
            "remaining payload should have more nodes"
        );
    }

    #[test]
    fn declare_commands_simple_no_args() {
        let commands = vec![basalt_api::CommandEntry {
            name: "help".into(),
            description: "Show help".into(),
            args: Vec::new(),
            variants: Vec::new(),
            handler: Box::new(|_args, _ctx| {}),
        }];

        let (payload, count) = build_declare_commands(&commands);
        assert_eq!(count, 1);

        let mut cursor: &[u8] = &payload;
        let node_count = basalt_types::VarInt::decode(&mut cursor).unwrap().0;
        assert_eq!(node_count, 2); // root + literal

        // Root
        let _flags = u8::decode(&mut cursor).unwrap();
        let _child_count = basalt_types::VarInt::decode(&mut cursor).unwrap().0;
        let _child = basalt_types::VarInt::decode(&mut cursor).unwrap().0;

        // Literal "help" — executable, no children
        let flags = u8::decode(&mut cursor).unwrap();
        assert_eq!(flags, 0x05); // literal(1) + executable(4)
        let child_count = basalt_types::VarInt::decode(&mut cursor).unwrap().0;
        assert_eq!(child_count, 0);
    }

    #[test]
    fn declare_commands_gamemode_options() {
        let commands = vec![basalt_api::CommandEntry {
            name: "gamemode".into(),
            description: "Change gamemode".into(),
            args: vec![basalt_command::CommandArg {
                name: "mode".into(),
                arg_type: basalt_command::Arg::Options(vec!["survival".into(), "creative".into()]),
                validation: basalt_command::Validation::Auto,
                required: true,
            }],
            variants: Vec::new(),
            handler: Box::new(|_args, _ctx| {}),
        }];

        let (payload, count) = build_declare_commands(&commands);
        assert_eq!(count, 1);

        let mut cursor: &[u8] = &payload;
        let node_count = basalt_types::VarInt::decode(&mut cursor).unwrap().0;
        // root + literal "gamemode" + argument "mode"
        assert_eq!(node_count, 3);
    }
}
