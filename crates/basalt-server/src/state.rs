//! Shared server state and command tree builder.
//!
//! `ServerState` holds the minimal shared state passed to connection
//! tasks: entity ID counter, world reference, and command metadata.
//! Player management and event dispatch are handled by the network
//! and game loops, not by `ServerState`.

use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

pub(crate) use basalt_api::EventBus;

/// Shared server state, held behind `Arc` and passed to connection tasks.
///
/// After the two-loop architecture, this struct is intentionally minimal.
/// Player registration, broadcasting, and event dispatch live in the
/// network and game loops. `ServerState` only holds data needed by
/// the connection setup flow and shared across loops.
pub(crate) struct ServerState {
    /// Shared counter for assigning unique entity IDs.
    ///
    /// Behind `Arc` so the game loop can share it for spawning
    /// non-player entities (dropped items, mobs).
    next_entity_id: Arc<AtomicI32>,
    /// The world — chunk cache and terrain generator.
    pub world: Arc<basalt_world::World>,
    /// Pre-built DeclareCommands packet payload (empty if no commands).
    pub declare_commands: Vec<u8>,
    /// Command arg metadata for TabComplete suggestions.
    pub command_args: Vec<CommandMeta>,
}

/// Command metadata for TabComplete suggestions.
///
/// Stores only the argument schema — not the handler.
#[derive(Clone)]
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

impl ServerState {
    /// Builds the server state and returns the event buses separately.
    ///
    /// The buses are moved into the network and game loops. `ServerState`
    /// retains only world access, entity ID counter, and command metadata.
    pub fn build_for_loops(
        world: Arc<basalt_world::World>,
        plugins: Vec<Box<dyn basalt_api::Plugin>>,
    ) -> (
        Arc<Self>,
        EventBus,
        EventBus,
        Vec<basalt_core::SystemDescriptor>,
        basalt_recipes::RecipeRegistry,
    ) {
        let mut instant_bus = EventBus::new();
        let mut game_bus = EventBus::new();
        let mut commands = Vec::new();
        let mut systems = Vec::new();
        let mut recipes = basalt_recipes::RecipeRegistry::with_vanilla();
        {
            let mut registrar = basalt_api::PluginRegistrar::new(
                &mut instant_bus,
                &mut game_bus,
                &mut commands,
                &mut systems,
                std::sync::Arc::clone(&world),
                &mut recipes,
            );
            for plugin in &plugins {
                log::info!(target: "basalt::plugin", "Enabling {} v{}", plugin.metadata().name, plugin.metadata().version);
                plugin.on_enable(&mut registrar);
            }
        }

        let declare_commands = build_declare_commands(&commands);
        let command_args: Vec<CommandMeta> = commands
            .iter()
            .map(|c| CommandMeta {
                name: c.name.clone(),
                description: c.description.clone(),
                args: c.args.clone(),
                variants: c.variants.clone(),
            })
            .collect();

        // Register command dispatch on the network bus
        if !commands.is_empty() {
            let commands: Vec<basalt_api::CommandEntry> = commands.into_iter().collect();
            let commands = Arc::new(commands);
            instant_bus.on::<basalt_api::events::CommandEvent, basalt_api::context::ServerContext>(
                basalt_events::Stage::Process,
                -100,
                move |event, ctx| {
                    use basalt_core::Context;
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
                                ctx.chat().send_component(
                                    &basalt_types::TextComponent::text(err_msg).color(
                                        basalt_types::TextColor::Named(
                                            basalt_types::NamedColor::Red,
                                        ),
                                    ),
                                );
                            }
                        }
                    } else {
                        ctx.chat().send_component(
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

        let state = Arc::new(Self {
            next_entity_id: Arc::new(AtomicI32::new(1)),
            world,
            declare_commands: declare_commands.0,
            command_args,
        });

        (state, instant_bus, game_bus, systems, recipes)
    }

    /// Allocates a unique entity ID for a new player.
    pub fn next_entity_id(&self) -> i32 {
        self.next_entity_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Returns a shared reference to the entity ID counter.
    ///
    /// The game loop uses this to assign entity IDs for non-player
    /// entities (dropped items, mobs) using the same counter as
    /// player entities, avoiding ID collisions.
    pub fn entity_id_counter(&self) -> Arc<AtomicI32> {
        Arc::clone(&self.next_entity_id)
    }
}

/// Builds the raw DeclareCommands packet payload from registered commands.
///
/// Generates a Brigadier command tree with argument nodes for
/// tab-completion. Commands with variants get multiple child branches.
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
            VarInt(0).encode(&mut data).unwrap();
        }
        Arg::Entity => {
            VarInt(6).encode(&mut data).unwrap();
            0x00u8.encode(&mut data).unwrap();
        }
        Arg::Player => {
            VarInt(6).encode(&mut data).unwrap();
            0x01u8.encode(&mut data).unwrap();
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
fn build_arg_trie(
    chains: &[&[basalt_command::CommandArg]],
    nodes: &mut Vec<BrigNode>,
    parent_children: &mut Vec<i32>,
) {
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
        let has_suggestions = false;
        let is_executable = group.iter().any(|c| c.len() == 1);

        let mut flags: u8 = 0x02;
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

    nodes.push(BrigNode {
        flags: 0x00,
        children: Vec::new(),
        name: None,
        parser: None,
        suggestions: None,
    });

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
            nodes[literal_idx].flags |= 0x04;
            continue;
        };

        let chains: Vec<&[basalt_command::CommandArg]> =
            arg_lists.iter().map(|v| v.as_slice()).collect();

        let mut children = Vec::new();
        build_arg_trie(&chains, &mut nodes, &mut children);
        nodes[literal_idx].children = children;
    }

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

    VarInt(0).encode(&mut buf).unwrap();
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

    use super::*;

    fn test_state() -> Arc<ServerState> {
        let config = crate::config::ServerConfig::default();
        let world = Arc::new(config.create_world());
        let plugins = config.create_plugins();
        let (state, _instant_bus, _game_bus, _systems, _recipes) =
            ServerState::build_for_loops(world, plugins);
        state
    }

    #[test]
    fn next_entity_id_increments() {
        let state = test_state();
        assert_eq!(state.next_entity_id(), 1);
        assert_eq!(state.next_entity_id(), 2);
        assert_eq!(state.next_entity_id(), 3);
    }

    #[test]
    fn declare_commands_tp_has_three_variants() {
        use basalt_command::{Arg, CommandArg, Validation};

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
        assert_eq!(node_count, 7, "expected 7 nodes in tp tree");

        let flags = u8::decode(&mut cursor).unwrap();
        assert_eq!(flags, 0x00);
        let child_count = basalt_types::VarInt::decode(&mut cursor).unwrap().0;
        assert_eq!(child_count, 1);
        let _tp_idx = basalt_types::VarInt::decode(&mut cursor).unwrap().0;

        let flags = u8::decode(&mut cursor).unwrap();
        assert_eq!(flags & 0x03, 0x01);
        assert_eq!(flags & 0x04, 0x00);
        let child_count = basalt_types::VarInt::decode(&mut cursor).unwrap().0;
        assert_eq!(
            child_count, 3,
            "tp should have 3 children: destination, location, targets"
        );
        let dest_idx = basalt_types::VarInt::decode(&mut cursor).unwrap().0;
        let _loc_idx = basalt_types::VarInt::decode(&mut cursor).unwrap().0;
        let _tgt_idx = basalt_types::VarInt::decode(&mut cursor).unwrap().0;
        let name_len = basalt_types::VarInt::decode(&mut cursor).unwrap().0 as usize;
        let mut name_buf = vec![0u8; name_len];
        name_buf.copy_from_slice(&cursor[..name_len]);
        cursor = &cursor[name_len..];
        assert_eq!(std::str::from_utf8(&name_buf).unwrap(), "tp");

        let flags = u8::decode(&mut cursor).unwrap();
        assert_eq!(flags & 0x03, 0x02, "destination should be argument type");
        assert_ne!(flags & 0x04, 0, "destination should be executable");
        assert_eq!(dest_idx, 2);
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
        assert_eq!(node_count, 2);

        let _flags = u8::decode(&mut cursor).unwrap();
        let _child_count = basalt_types::VarInt::decode(&mut cursor).unwrap().0;
        let _child = basalt_types::VarInt::decode(&mut cursor).unwrap().0;

        let flags = u8::decode(&mut cursor).unwrap();
        assert_eq!(flags, 0x05);
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
        assert_eq!(node_count, 3);
    }
}
