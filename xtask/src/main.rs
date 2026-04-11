//! Code generation tool for Minecraft protocol packets.
//!
//! Reads packet definitions from the minecraft-data JSON files and generates
//! Rust source code with `#[packet(id)]` attribute macros and typed fields.
//!
//! Usage: `cargo xt codegen`

use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;

/// The Minecraft version to generate packets for.
const VERSION: &str = "1.21.4";

/// Path to the minecraft-data submodule relative to the workspace root.
const MINECRAFT_DATA_PATH: &str = "minecraft-data/data/pc";

/// Output directory for generated packets relative to the workspace root.
const PACKETS_DIR: &str = "crates/basalt-protocol/src/packets";

/// Protocol states to generate, mapped to their JSON key and Rust module name.
const STATES: &[(&str, &str)] = &[
    ("handshaking", "handshake"),
    ("status", "status"),
    ("login", "login"),
    ("configuration", "configuration"),
    ("play", "play"),
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("codegen") => codegen(),
        _ => {
            eprintln!("Usage: cargo xt codegen");
            std::process::exit(1);
        }
    }
}

/// Runs the code generation pipeline for all configured states.
fn codegen() {
    let workspace_root = find_workspace_root();
    let protocol_path = workspace_root
        .join(MINECRAFT_DATA_PATH)
        .join(VERSION)
        .join("protocol.json");

    println!("Reading protocol data from {}", protocol_path.display());
    let protocol_json = fs::read_to_string(&protocol_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", protocol_path.display()));

    let protocol: Value = serde_json::from_str(&protocol_json).expect("Failed to parse JSON");

    for &(json_key, module_name) in STATES {
        let state_data = &protocol[json_key];
        if state_data.is_null() {
            eprintln!("Warning: state '{json_key}' not found in protocol.json, skipping");
            continue;
        }

        if module_name == "play" {
            generate_play_split(state_data, &workspace_root);
        } else {
            let code = generate_state_module(state_data, module_name);
            let output_path = workspace_root
                .join(PACKETS_DIR)
                .join(format!("{module_name}.rs"));
            println!("Writing {module_name} packets to {}", output_path.display());
            fs::write(&output_path, &code)
                .unwrap_or_else(|e| panic!("Failed to write {}: {e}", output_path.display()));
            format_file(&output_path);
        }
    }

    // Generate packets/mod.rs from files on disk
    let mod_path = workspace_root.join(PACKETS_DIR).join("mod.rs");
    println!("Writing packets mod.rs to {}", mod_path.display());
    let mod_code = generate_packets_mod(&workspace_root);
    fs::write(&mod_path, mod_code)
        .unwrap_or_else(|e| panic!("Failed to write {}: {e}", mod_path.display()));
    format_file(&mod_path);

    println!("Done.");
}

/// Runs rustfmt on a generated file to ensure it matches the project's
/// formatting standards. This way the codegen output is commit-ready
/// without a separate `cargo fmt` step.
fn format_file(path: &std::path::Path) {
    let status = std::process::Command::new("rustfmt")
        .arg("--edition")
        .arg("2024")
        .arg(path)
        .status()
        .unwrap_or_else(|e| panic!("Failed to run rustfmt on {}: {e}", path.display()));
    if !status.success() {
        eprintln!(
            "Warning: rustfmt failed on {} (exit code {:?})",
            path.display(),
            status.code()
        );
    }
}

/// Finds the workspace root by looking for Cargo.toml with [workspace].
fn find_workspace_root() -> std::path::PathBuf {
    let mut dir = std::env::current_dir().expect("Failed to get current directory");
    loop {
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.exists() {
            let content = fs::read_to_string(&cargo_toml).unwrap_or_default();
            if content.contains("[workspace]") {
                return dir;
            }
        }
        if !dir.pop() {
            panic!("Could not find workspace root (no Cargo.toml with [workspace])");
        }
    }
}

/// Generates the packets/mod.rs file by scanning existing .rs files.
fn generate_packets_mod(workspace_root: &std::path::Path) -> String {
    let packets_dir = workspace_root.join(PACKETS_DIR);
    let mut modules: Vec<String> = fs::read_dir(&packets_dir)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", packets_dir.display()))
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".rs") && name != "mod.rs" {
                Some(name.strip_suffix(".rs").unwrap().to_string())
            } else {
                None
            }
        })
        .collect();
    modules.sort();

    let mut out = String::new();
    out.push_str("//! Minecraft packet definitions organized by connection state.\n");
    out.push_str("//!\n");
    out.push_str("//! Each submodule contains the packet structs for one connection state,\n");
    out.push_str("//! along with direction-specific enums (serverbound/clientbound) that\n");
    out.push_str("//! enable exhaustive pattern matching on received packets.\n");
    out.push_str("//!\n");
    out.push_str("//! Auto-generated by `cargo xt codegen`. Do not edit manually.\n\n");
    for module in &modules {
        out.push_str(&format!("pub mod {module};\n"));
    }
    out
}

/// Generates a complete Rust module for one protocol state.
fn generate_state_module(state: &Value, module_name: &str) -> String {
    let mut output = String::new();

    let pascal_state = to_pascal_case(module_name);

    // Module header
    output.push_str(&format!("//! {pascal_state} state packet definitions.\n"));
    output.push_str("//!\n");
    output.push_str("//! Auto-generated by `cargo xt codegen` from minecraft-data.\n");
    output.push_str("//! Do not edit manually — changes will be overwritten.\n\n");
    let serverbound = parse_direction(state, "toServer", "Serverbound", &pascal_state);
    let clientbound = parse_direction(state, "toClient", "Clientbound", &pascal_state);

    let has_inline_structs = serverbound
        .iter()
        .chain(clientbound.iter())
        .any(|p| !p.inline_structs.is_empty());

    // Collect all type names used across all packets to generate imports
    let all_types: Vec<&str> = serverbound
        .iter()
        .chain(clientbound.iter())
        .flat_map(|p| {
            p.fields
                .iter()
                .chain(p.inline_structs.iter().flat_map(|s| s.fields.iter()))
                .map(|f| f.rust_type.as_str())
        })
        .collect();

    let needs = |name: &str| all_types.iter().any(|t| t.contains(name));

    if has_inline_structs {
        output.push_str("use basalt_derive::{packet, Encode, Decode, EncodedSize};\n");
    } else {
        output.push_str("use basalt_derive::packet;\n");
    }
    output.push_str("use basalt_types::Decode as _;\n");

    // Import basalt-types types that are used in generated packets
    let mut basalt_imports = Vec::new();
    for (type_name, import_name) in [
        ("Uuid", "Uuid"),
        ("Position", "Position"),
        ("NbtCompound", "NbtCompound"),
        ("Slot", "Slot"),
        ("Vec2f", "Vec2f"),
        ("Vec3f64", "Vec3f64"),
        ("Vec3f", "Vec3f"),
        ("Vec3i16", "Vec3i16"),
    ] {
        if needs(type_name) {
            basalt_imports.push(import_name);
        }
    }
    if !basalt_imports.is_empty() {
        output.push_str(&format!(
            "use basalt_types::{{{}}};\n",
            basalt_imports.join(", ")
        ));
    }

    output.push_str("\nuse crate::error::{Error, Result};\n\n");

    // Generate serverbound packet structs
    if !serverbound.is_empty() {
        output.push_str("// -- Serverbound packets --\n\n");
        for packet in &serverbound {
            output.push_str(&generate_packet_struct(packet));
            output.push('\n');
        }
    }

    // Generate clientbound packet structs
    if !clientbound.is_empty() {
        output.push_str("// -- Clientbound packets --\n\n");
        for packet in &clientbound {
            output.push_str(&generate_packet_struct(packet));
            output.push('\n');
        }
    }

    // Generate direction enums
    if !serverbound.is_empty() {
        output.push_str(&generate_direction_enum(
            &format!("Serverbound{pascal_state}Packet"),
            &serverbound,
            module_name,
            &pascal_state,
        ));
        output.push('\n');
    }

    if !clientbound.is_empty() {
        output.push_str(&generate_direction_enum(
            &format!("Clientbound{pascal_state}Packet"),
            &clientbound,
            module_name,
            &pascal_state,
        ));
    }

    // Generate tests
    output.push('\n');
    output.push_str(&generate_tests(
        &serverbound,
        &clientbound,
        &pascal_state,
        module_name,
    ));

    output
}

// -- Data structures --

#[derive(Debug)]
struct PacketDef {
    name: String,
    id: String,
    fields: Vec<FieldDef>,
    inline_structs: Vec<InlineStruct>,
}

#[derive(Debug)]
struct FieldDef {
    name: String,
    rust_type: String,
    attribute: Option<String>,
}

#[derive(Debug)]
struct InlineStruct {
    name: String,
    fields: Vec<FieldDef>,
}

// -- Parsing --

fn parse_direction(
    state: &Value,
    direction: &str,
    dir_prefix: &str,
    state_pascal: &str,
) -> Vec<PacketDef> {
    let dir_data = &state[direction];
    if dir_data.is_null() {
        return Vec::new();
    }

    let types = &dir_data["types"];
    let packet_mapper = &types["packet"];
    if packet_mapper.is_null() {
        return Vec::new();
    }

    let mappings = &packet_mapper[1][0]["type"][1]["mappings"];
    let id_map: BTreeMap<String, String> = mappings
        .as_object()
        .expect("mappings should be an object")
        .iter()
        .map(|(id, name)| (name.as_str().unwrap().to_string(), id.clone()))
        .collect();

    let mut packets = Vec::new();
    for (name, id) in &id_map {
        let type_key = format!("packet_{name}");

        // Check if the switch redirects to a common packet
        let switch_fields = &packet_mapper[1][1]["type"][1]["fields"];
        let actual_type_key = switch_fields[name.as_str()].as_str().unwrap_or(&type_key);

        if actual_type_key.starts_with("packet_common_") {
            continue;
        }

        let packet_type = &types[actual_type_key];
        if packet_type.is_null() {
            eprintln!("Warning: packet type {actual_type_key} not found, skipping");
            continue;
        }

        let struct_name = format!("{dir_prefix}{state_pascal}{}", to_pascal_case(name));
        let (fields, inline_structs) = parse_container_fields(packet_type, &struct_name);

        packets.push(PacketDef {
            name: struct_name,
            id: id.clone(),
            fields,
            inline_structs,
        });
    }

    packets
}

fn parse_container_fields(
    container: &Value,
    parent_name: &str,
) -> (Vec<FieldDef>, Vec<InlineStruct>) {
    let mut fields = Vec::new();
    let mut inline_structs = Vec::new();

    let field_array = container[1]
        .as_array()
        .expect("container fields should be an array");

    for (i, field) in field_array.iter().enumerate() {
        let field_name = field["name"]
            .as_str()
            .expect("field should have a name")
            .to_string();
        let rust_name = to_snake_case(&field_name);
        let field_type = &field["type"];
        let is_last = i == field_array.len() - 1;

        let (rust_type, attribute, inline) =
            map_type(field_type, parent_name, &field_name, is_last);

        if let Some(inline_struct) = inline {
            inline_structs.push(inline_struct);
        }

        fields.push(FieldDef {
            name: rust_name,
            rust_type,
            attribute,
        });
    }

    (fields, inline_structs)
}

fn map_type(
    type_def: &Value,
    parent_name: &str,
    field_name: &str,
    is_last: bool,
) -> (String, Option<String>, Option<InlineStruct>) {
    match type_def {
        Value::String(s) => match s.as_str() {
            "varint" => ("i32".into(), Some("varint".into()), None),
            "varlong" => ("i64".into(), Some("varlong".into()), None),
            "string" => ("String".into(), None, None),
            "bool" => ("bool".into(), None, None),
            "u8" => ("u8".into(), None, None),
            "u16" => ("u16".into(), None, None),
            "u32" => ("u32".into(), None, None),
            "u64" => ("u64".into(), None, None),
            "i8" => ("i8".into(), None, None),
            "i16" => ("i16".into(), None, None),
            "i32" => ("i32".into(), None, None),
            "i64" => ("i64".into(), None, None),
            "f32" => ("f32".into(), None, None),
            "f64" => ("f64".into(), None, None),
            "UUID" => ("Uuid".into(), None, None),
            "position" => ("Position".into(), None, None),
            "anonymousNbt" => ("NbtCompound".into(), None, None),
            "anonOptionalNbt" => ("Option<NbtCompound>".into(), Some("optional".into()), None),
            "ByteArray" => ("Vec<u8>".into(), Some("length = \"varint\"".into()), None),
            "ContainerID" | "optvarint" => ("i32".into(), Some("varint".into()), None),
            "soundSource" => ("i32".into(), Some("varint".into()), None),
            "Slot" => ("Slot".into(), None, None),
            "vec2f" => ("Vec2f".into(), None, None),
            "vec3f" => ("Vec3f".into(), None, None),
            "vec3f64" => ("Vec3f64".into(), None, None),
            "vec3i16" => ("Vec3i16".into(), None, None),
            "packedChunkPos" => ("i64".into(), None, None),
            "restBuffer" => {
                if is_last {
                    ("Vec<u8>".into(), Some("rest".into()), None)
                } else {
                    ("Vec<u8>".into(), None, None)
                }
            }
            other => {
                eprintln!("Warning: unknown type '{other}', using Vec<u8>");
                ("Vec<u8>".into(), None, None)
            }
        },
        Value::Array(arr) if arr.len() == 2 => {
            let type_name = arr[0].as_str().unwrap_or("");
            match type_name {
                "buffer" => {
                    let count_type = arr[1]["countType"].as_str().unwrap_or("varint");
                    if count_type == "varint" {
                        ("Vec<u8>".into(), Some("length = \"varint\"".into()), None)
                    } else {
                        ("Vec<u8>".into(), None, None)
                    }
                }
                "option" => {
                    let inner_type = &arr[1];
                    let (inner_rust, _inner_attr, inline) =
                        map_type(inner_type, parent_name, field_name, is_last);
                    // If the inner type produced an inline struct, we can't
                    // wrap it in Option because Vec<InlineStruct> doesn't
                    // implement Encode/Decode. Fall back to opaque bytes.
                    if inline.is_some() {
                        ("Option<Vec<u8>>".into(), Some("optional".into()), None)
                    } else {
                        (
                            format!("Option<{inner_rust}>"),
                            Some("optional".into()),
                            None,
                        )
                    }
                }
                "array" => {
                    let count_type = arr[1]["countType"].as_str().unwrap_or("varint");
                    let inner_type = &arr[1]["type"];

                    if inner_type.is_array() && inner_type[0].as_str() == Some("container") {
                        let struct_name = format!("{}{}", parent_name, to_pascal_case(field_name));
                        let (inner_fields, nested_inlines) =
                            parse_container_fields(inner_type, &struct_name);

                        // For nested inline structs (arrays inside arrays),
                        // we need to handle them as Vec<u8> because our derive
                        // system doesn't support nested generic containers.
                        // The top-level inline struct fields that reference
                        // nested inlines will use Vec<u8> fallback.
                        let clean_fields = if nested_inlines.is_empty() {
                            inner_fields
                        } else {
                            // Replace fields that reference nested inlines
                            // with Vec<u8> — we can't derive Encode/Decode
                            // for Vec<NestedInlineStruct>
                            inner_fields
                                .into_iter()
                                .map(|f| {
                                    if nested_inlines
                                        .iter()
                                        .any(|ni| f.rust_type.contains(&ni.name))
                                    {
                                        FieldDef {
                                            name: f.name,
                                            rust_type: "Vec<u8>".into(),
                                            attribute: Some("length = \"varint\"".into()),
                                        }
                                    } else {
                                        f
                                    }
                                })
                                .collect()
                        };

                        let inline = InlineStruct {
                            name: struct_name.clone(),
                            fields: clean_fields,
                        };

                        if count_type == "varint" {
                            (
                                format!("Vec<{struct_name}>"),
                                Some("length = \"varint\"".into()),
                                Some(inline),
                            )
                        } else {
                            (format!("Vec<{struct_name}>"), None, Some(inline))
                        }
                    } else {
                        let (inner_rust, _, inline) =
                            map_type(inner_type, parent_name, field_name, false);
                        if count_type == "varint" {
                            (
                                format!("Vec<{inner_rust}>"),
                                Some("length = \"varint\"".into()),
                                inline,
                            )
                        } else {
                            (format!("Vec<{inner_rust}>"), None, inline)
                        }
                    }
                }
                _ => {
                    eprintln!("Warning: unknown compound type '{type_name}', using Vec<u8>");
                    ("Vec<u8>".into(), None, None)
                }
            }
        }
        _ => {
            eprintln!("Warning: unexpected type format, using Vec<u8>");
            ("Vec<u8>".into(), None, None)
        }
    }
}

// -- Code generation --

fn generate_packet_struct(packet: &PacketDef) -> String {
    let mut out = String::new();

    for inline in &packet.inline_structs {
        out.push_str(&format!(
            "/// Inline data structure used by [`{}`].\n",
            packet.name
        ));
        out.push_str("#[derive(Debug, Clone, Default, PartialEq, Encode, Decode, EncodedSize)]\n");
        out.push_str(&format!("pub struct {} {{\n", inline.name));
        for field in &inline.fields {
            if let Some(attr) = &field.attribute {
                out.push_str(&format!("    #[field({attr})]\n"));
            }
            out.push_str(&format!("    pub {}: {},\n", field.name, field.rust_type));
        }
        out.push_str("}\n\n");
    }

    if packet.fields.is_empty() {
        out.push_str("#[derive(Debug, Clone, Default, PartialEq)]\n");
        out.push_str(&format!("#[packet(id = {})]\n", packet.id));
        out.push_str(&format!("pub struct {};\n", packet.name));
    } else {
        out.push_str("#[derive(Debug, Clone, Default, PartialEq)]\n");
        out.push_str(&format!("#[packet(id = {})]\n", packet.id));
        out.push_str(&format!("pub struct {} {{\n", packet.name));
        for field in &packet.fields {
            if let Some(attr) = &field.attribute {
                out.push_str(&format!("    #[field({attr})]\n"));
            }
            out.push_str(&format!("    pub {}: {},\n", field.name, field.rust_type));
        }
        out.push_str("}\n");
    }

    out
}

fn generate_direction_enum(
    enum_name: &str,
    packets: &[PacketDef],
    state_name: &str,
    state_pascal: &str,
) -> String {
    let mut out = String::new();

    let direction = if enum_name.starts_with("Serverbound") {
        "serverbound"
    } else {
        "clientbound"
    };
    out.push_str(&format!(
        "/// {direction} packets in the {state_pascal} state.\n"
    ));
    out.push_str("#[derive(Debug, Clone, PartialEq)]\n");
    out.push_str(&format!("pub enum {enum_name} {{\n"));
    for packet in packets {
        let variant = short_variant_name(&packet.name, state_pascal);
        out.push_str(&format!("    {variant}({}),\n", packet.name));
    }
    out.push_str("}\n\n");

    out.push_str(&format!("impl {enum_name} {{\n"));
    out.push_str("    /// Decodes a packet from its ID and payload.\n");
    out.push_str("    pub fn decode_by_id(id: i32, buf: &mut &[u8]) -> Result<Self> {\n");
    out.push_str("        match id {\n");
    for packet in packets {
        let variant = short_variant_name(&packet.name, state_pascal);
        out.push_str(&format!(
            "            {}::PACKET_ID => Ok(Self::{variant}({}::decode(buf)?)),\n",
            packet.name, packet.name
        ));
    }
    out.push_str(&format!(
        "            _ => Err(Error::UnknownPacket {{ id, state: \"{state_name}\" }}),\n"
    ));
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("}\n");

    out
}

fn generate_tests(
    serverbound: &[PacketDef],
    clientbound: &[PacketDef],
    state_pascal: &str,
    _state_name: &str,
) -> String {
    let mut out = String::new();
    out.push_str("#[cfg(test)]\n");
    out.push_str("mod tests {\n");
    out.push_str("    use super::*;\n");
    out.push_str("    use basalt_types::{Encode as _, EncodedSize as _};\n\n");

    // Packet ID tests
    if !serverbound.is_empty() {
        out.push_str("    #[test]\n");
        out.push_str("    fn serverbound_packet_ids() {\n");
        for packet in serverbound {
            out.push_str(&format!(
                "        assert_eq!({}::PACKET_ID, {});\n",
                packet.name, packet.id
            ));
        }
        out.push_str("    }\n\n");

        out.push_str("    #[test]\n");
        out.push_str("    fn unknown_serverbound_id() {\n");
        out.push_str("        let mut cursor: &[u8] = &[];\n");
        out.push_str(&format!(
            "        assert!(Serverbound{state_pascal}Packet::decode_by_id(0xFF, &mut cursor).is_err());\n"
        ));
        out.push_str("    }\n\n");
    }

    if !clientbound.is_empty() {
        out.push_str("    #[test]\n");
        out.push_str("    fn clientbound_packet_ids() {\n");
        for packet in clientbound {
            out.push_str(&format!(
                "        assert_eq!({}::PACKET_ID, {});\n",
                packet.name, packet.id
            ));
        }
        out.push_str("    }\n\n");

        out.push_str("    #[test]\n");
        out.push_str("    fn unknown_clientbound_id() {\n");
        out.push_str("        let mut cursor: &[u8] = &[];\n");
        out.push_str(&format!(
            "        assert!(Clientbound{state_pascal}Packet::decode_by_id(0xFF, &mut cursor).is_err());\n"
        ));
        out.push_str("    }\n\n");
    }

    // Roundtrip tests for each packet
    for packet in serverbound.iter().chain(clientbound.iter()) {
        let test_name = to_snake_case(&packet.name);
        let constructor = if packet.fields.is_empty() {
            // Unit struct — use the struct name directly
            packet.name.clone()
        } else {
            format!("{}::default()", packet.name)
        };
        out.push_str("    #[test]\n");
        out.push_str(&format!("    fn {test_name}_roundtrip() {{\n"));
        out.push_str(&format!("        let original = {constructor};\n"));
        out.push_str("        let mut buf = Vec::with_capacity(original.encoded_size());\n");
        out.push_str("        original.encode(&mut buf).unwrap();\n");
        out.push_str("        assert_eq!(buf.len(), original.encoded_size());\n");
        out.push_str("        let mut cursor = buf.as_slice();\n");
        out.push_str(&format!(
            "        let decoded = {}::decode(&mut cursor).unwrap();\n",
            packet.name
        ));
        out.push_str("        assert!(cursor.is_empty());\n");
        out.push_str("        assert_eq!(decoded, original);\n");
        out.push_str("    }\n\n");
    }

    out.push_str("}\n");
    out
}

// -- Play split by category --

/// Play packet categories. Each packet is assigned to a category based
/// on its name. This determines which sub-file it goes into.
const PLAY_CATEGORIES: &[(&str, &[&str])] = &[
    (
        "entity",
        &[
            "spawn_entity",
            "spawn_entity_experience_orb",
            "animation",
            "entity_status",
            "entity_metadata",
            "entity_destroy",
            "entity_velocity",
            "entity_equipment",
            "entity_head_rotation",
            "entity_look",
            "entity_move_look",
            "rel_entity_move",
            "entity_teleport",
            "entity_sound_effect",
            "entity_effect",
            "remove_entity_effect",
            "entity_update_attributes",
            "attach_entity",
            "set_passengers",
            "collect",
            "use_entity",
            "entity_action",
            "arm_animation",
            "damage_event",
            "hurt_animation",
            "sync_entity_position",
            "move_minecart",
            "set_projectile_power",
            "query_entity_nbt",
            "block_break_animation",
        ],
    ),
    (
        "world",
        &[
            "block_change",
            "multi_block_change",
            "block_action",
            "map_chunk",
            "chunk_batch_finished",
            "chunk_batch_start",
            "chunk_batch_received",
            "chunk_biomes",
            "unload_chunk",
            "update_light",
            "map",
            "explosion",
            "world_event",
            "world_particles",
            "world_border_center",
            "world_border_lerp_size",
            "world_border_size",
            "world_border_warning_delay",
            "world_border_warning_reach",
            "initialize_world_border",
            "update_time",
            "spawn_position",
            "update_view_position",
            "update_view_distance",
            "tile_entity_data",
            "block_dig",
            "block_place",
            "acknowledge_player_digging",
            "query_block_nbt",
            "nbt_query_response",
            "generate_structure",
            "update_sign",
        ],
    ),
    (
        "player",
        &[
            "position",
            "position_look",
            "look",
            "flying",
            "vehicle_move",
            "steer_boat",
            "abilities",
            "player_info",
            "player_remove",
            "player_chat",
            "player_rotation",
            "game_state_change",
            "respawn",
            "experience",
            "update_health",
            "face_player",
            "camera",
            "spectate",
            "teleport_confirm",
            "client_command",
            "settings",
            "login",
            "difficulty",
            "set_difficulty",
            "lock_difficulty",
            "end_combat_event",
            "enter_combat_event",
            "death_combat_event",
            "simulation_distance",
            "player_input",
            "player_loaded",
        ],
    ),
    (
        "inventory",
        &[
            "window_click",
            "window_items",
            "close_window",
            "open_window",
            "open_horse_window",
            "set_slot",
            "set_slot_state",
            "craft_progress_bar",
            "craft_recipe_request",
            "craft_recipe_response",
            "set_creative_slot",
            "held_item_slot",
            "set_cooldown",
            "trade_list",
            "select_trade",
            "enchant_item",
            "set_beacon_effect",
            "pick_item_from_block",
            "pick_item_from_entity",
            "name_item",
            "select_bundle_item",
            "open_book",
            "open_sign_entity",
            "set_cursor_item",
            "set_player_inventory",
            "collect",
        ],
    ),
    (
        "chat",
        &[
            "chat_message",
            "chat_command",
            "chat_command_signed",
            "chat_session_update",
            "chat_suggestions",
            "system_chat",
            "profileless_chat",
            "hide_message",
            "message_acknowledgement",
            "tab_complete",
            "declare_commands",
            "action_bar",
            "set_title_text",
            "set_title_subtitle",
            "set_title_time",
            "clear_titles",
            "playerlist_header",
            "scoreboard_objective",
            "scoreboard_display_objective",
            "scoreboard_score",
            "reset_score",
            "teams",
            "boss_bar",
        ],
    ),
];

/// Determines the category for a play packet by its short name.
fn play_category(packet_short_name: &str) -> &'static str {
    for &(category, names) in PLAY_CATEGORIES {
        if names.contains(&packet_short_name) {
            return category;
        }
    }
    "misc"
}

/// Generates the play state as a directory with category sub-files.
fn generate_play_split(state: &Value, workspace_root: &std::path::Path) {
    let play_dir = workspace_root.join(PACKETS_DIR).join("play");
    fs::create_dir_all(&play_dir)
        .unwrap_or_else(|e| panic!("Failed to create {}: {e}", play_dir.display()));

    let pascal_state = "Play";
    let serverbound = parse_direction(state, "toServer", "Serverbound", pascal_state);
    let clientbound = parse_direction(state, "toClient", "Clientbound", pascal_state);

    // Categorize packets
    let mut categories: BTreeMap<&str, (Vec<&PacketDef>, Vec<&PacketDef>)> = BTreeMap::new();
    for packet in &serverbound {
        let short = extract_play_short_name(&packet.name);
        let cat = play_category(&short);
        categories.entry(cat).or_default().0.push(packet);
    }
    for packet in &clientbound {
        let short = extract_play_short_name(&packet.name);
        let cat = play_category(&short);
        categories.entry(cat).or_default().1.push(packet);
    }

    // Generate a file per category
    for (&category, (sb, cb)) in &categories {
        let code = generate_category_file(category, sb, cb, pascal_state);
        let path = play_dir.join(format!("{category}.rs"));
        println!(
            "Writing play/{category}.rs ({} packets)",
            sb.len() + cb.len()
        );
        fs::write(&path, &code)
            .unwrap_or_else(|e| panic!("Failed to write {}: {e}", path.display()));
        format_file(&path);
    }

    // Generate play/mod.rs
    let mod_code = generate_play_mod(&categories, &serverbound, &clientbound);
    let mod_path = play_dir.join("mod.rs");
    println!("Writing play/mod.rs");
    fs::write(&mod_path, &mod_code)
        .unwrap_or_else(|e| panic!("Failed to write {}: {e}", mod_path.display()));
    format_file(&mod_path);
}

/// Extracts the short packet name from a full struct name.
/// e.g., "ServerboundPlayPosition" → "position"
/// e.g., "ClientboundPlayEntityMetadata" → "entity_metadata"
fn extract_play_short_name(full_name: &str) -> String {
    let without_dir = full_name
        .strip_prefix("Serverbound")
        .or_else(|| full_name.strip_prefix("Clientbound"))
        .unwrap_or(full_name);
    let without_state = without_dir.strip_prefix("Play").unwrap_or(without_dir);
    to_snake_case(without_state)
}

/// Generates a category sub-file with packet structs and their tests.
fn generate_category_file(
    category: &str,
    serverbound: &[&PacketDef],
    clientbound: &[&PacketDef],
    _state_pascal: &str,
) -> String {
    let mut output = String::new();

    output.push_str(&format!("//! Play state — {category} packets.\n"));
    output.push_str("//!\n");
    output.push_str("//! Auto-generated by `cargo xt codegen` from minecraft-data.\n");
    output.push_str("//! Do not edit manually — changes will be overwritten.\n\n");

    // Collect all packets to determine imports
    let all_packets: Vec<&PacketDef> = serverbound
        .iter()
        .chain(clientbound.iter())
        .copied()
        .collect();
    let has_inline = all_packets.iter().any(|p| !p.inline_structs.is_empty());

    let all_types: Vec<&str> = all_packets
        .iter()
        .flat_map(|p| {
            p.fields
                .iter()
                .chain(p.inline_structs.iter().flat_map(|s| s.fields.iter()))
                .map(|f| f.rust_type.as_str())
        })
        .collect();
    let needs = |name: &str| all_types.iter().any(|t| t.contains(name));

    if has_inline {
        output.push_str("use basalt_derive::{packet, Encode, Decode, EncodedSize};\n");
    } else {
        output.push_str("use basalt_derive::packet;\n");
    }

    let mut basalt_imports = Vec::new();
    for (type_name, import_name) in [
        ("Uuid", "Uuid"),
        ("Position", "Position"),
        ("NbtCompound", "NbtCompound"),
        ("Slot", "Slot"),
        ("Vec2f", "Vec2f"),
        ("Vec3f64", "Vec3f64"),
        ("Vec3f", "Vec3f"),
        ("Vec3i16", "Vec3i16"),
    ] {
        if needs(type_name) {
            basalt_imports.push(import_name);
        }
    }
    if !basalt_imports.is_empty() {
        output.push_str(&format!(
            "use basalt_types::{{{}}};\n",
            basalt_imports.join(", ")
        ));
    }
    output.push('\n');

    // Serverbound structs
    if !serverbound.is_empty() {
        output.push_str("// -- Serverbound packets --\n\n");
        for packet in serverbound {
            output.push_str(&generate_packet_struct(packet));
            output.push('\n');
        }
    }

    // Clientbound structs
    if !clientbound.is_empty() {
        output.push_str("// -- Clientbound packets --\n\n");
        for packet in clientbound {
            output.push_str(&generate_packet_struct(packet));
            output.push('\n');
        }
    }

    // Tests
    output.push_str("#[cfg(test)]\n");
    output.push_str("mod tests {\n");
    output.push_str("    use super::*;\n");
    output.push_str("    use basalt_types::{Encode as _, EncodedSize as _};\n\n");

    for packet in &all_packets {
        let test_name = to_snake_case(&packet.name);
        let constructor = if packet.fields.is_empty() {
            packet.name.clone()
        } else {
            format!("{}::default()", packet.name)
        };
        output.push_str("    #[test]\n");
        output.push_str(&format!("    fn {test_name}_roundtrip() {{\n"));
        output.push_str(&format!("        let original = {constructor};\n"));
        output.push_str("        let mut buf = Vec::with_capacity(original.encoded_size());\n");
        output.push_str("        original.encode(&mut buf).unwrap();\n");
        output.push_str("        assert_eq!(buf.len(), original.encoded_size());\n");
        output.push_str("        let mut cursor = buf.as_slice();\n");
        output.push_str(&format!(
            "        let decoded = {}::decode(&mut cursor).unwrap();\n",
            packet.name
        ));
        output.push_str("        assert!(cursor.is_empty());\n");
        output.push_str("        assert_eq!(decoded, original);\n");
        output.push_str("    }\n\n");
    }

    output.push_str("}\n");
    output
}

/// Generates the play/mod.rs that re-exports category modules and defines
/// the direction enums with decode_by_id spanning all categories.
fn generate_play_mod(
    categories: &BTreeMap<&str, (Vec<&PacketDef>, Vec<&PacketDef>)>,
    all_serverbound: &[PacketDef],
    all_clientbound: &[PacketDef],
) -> String {
    let mut out = String::new();
    out.push_str("//! Play state packet definitions, split by category.\n");
    out.push_str("//!\n");
    out.push_str("//! Auto-generated by `cargo xt codegen` from minecraft-data.\n");
    out.push_str("//! Do not edit manually — changes will be overwritten.\n\n");

    // Module declarations
    for category in categories.keys() {
        out.push_str(&format!("pub mod {category};\n"));
    }
    out.push('\n');

    // Re-exports
    for (&category, (sb, cb)) in categories {
        for packet in sb.iter().chain(cb.iter()) {
            out.push_str(&format!("pub use {category}::{};\n", packet.name));
            for inline in &packet.inline_structs {
                out.push_str(&format!("pub use {category}::{};\n", inline.name));
            }
        }
    }
    out.push('\n');

    out.push_str("use basalt_types::Decode as _;\n");
    out.push_str("use crate::error::{Error, Result};\n\n");

    // Direction enums
    out.push_str(&generate_direction_enum(
        "ServerboundPlayPacket",
        all_serverbound,
        "play",
        "Play",
    ));
    out.push('\n');
    out.push_str(&generate_direction_enum(
        "ClientboundPlayPacket",
        all_clientbound,
        "play",
        "Play",
    ));

    // ID tests
    out.push('\n');
    out.push_str("#[cfg(test)]\n");
    out.push_str("mod tests {\n");
    out.push_str("    use super::*;\n\n");

    out.push_str("    #[test]\n");
    out.push_str("    fn unknown_serverbound_id() {\n");
    out.push_str("        let mut cursor: &[u8] = &[];\n");
    out.push_str(
        "        assert!(ServerboundPlayPacket::decode_by_id(0xFF, &mut cursor).is_err());\n",
    );
    out.push_str("    }\n\n");

    out.push_str("    #[test]\n");
    out.push_str("    fn unknown_clientbound_id() {\n");
    out.push_str("        let mut cursor: &[u8] = &[];\n");
    out.push_str(
        "        assert!(ClientboundPlayPacket::decode_by_id(0xFF, &mut cursor).is_err());\n",
    );
    out.push_str("    }\n");

    out.push_str("}\n");
    out
}

// -- Helpers --

/// Removes the direction prefix and state name to get a short enum variant.
/// e.g., "ServerboundLoginEncryptionBegin" → "EncryptionBegin"
fn short_variant_name(full_name: &str, state_pascal: &str) -> String {
    let without_dir = full_name
        .strip_prefix("Serverbound")
        .or_else(|| full_name.strip_prefix("Clientbound"))
        .unwrap_or(full_name);
    without_dir
        .strip_prefix(state_pascal)
        .unwrap_or(without_dir)
        .to_string()
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().to_string() + &chars.as_str().to_lowercase(),
            }
        })
        .collect()
}

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    for i in 0..chars.len() {
        let c = chars[i];
        if c.is_uppercase() {
            let prev_is_lower = i > 0 && chars[i - 1].is_lowercase();
            let next_is_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();
            if prev_is_lower || (i > 0 && chars[i - 1].is_uppercase() && next_is_lower) {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap());
        } else {
            result.push(c);
        }
    }
    match result.as_str() {
        "type" => "r#type".to_string(),
        "match" => "r#match".to_string(),
        _ => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- to_snake_case --

    #[test]
    fn snake_case_simple() {
        assert_eq!(to_snake_case("serverHost"), "server_host");
    }

    #[test]
    fn snake_case_consecutive_uppercase() {
        assert_eq!(to_snake_case("playerUUID"), "player_uuid");
    }

    #[test]
    fn snake_case_all_lowercase() {
        assert_eq!(to_snake_case("username"), "username");
    }

    #[test]
    fn snake_case_leading_uppercase() {
        assert_eq!(to_snake_case("ServerHost"), "server_host");
    }

    #[test]
    fn snake_case_single_char() {
        assert_eq!(to_snake_case("x"), "x");
    }

    #[test]
    fn snake_case_keyword_type() {
        assert_eq!(to_snake_case("type"), "r#type");
    }

    #[test]
    fn snake_case_keyword_match() {
        assert_eq!(to_snake_case("match"), "r#match");
    }

    #[test]
    fn snake_case_camel_multi() {
        assert_eq!(to_snake_case("shouldAuthenticate"), "should_authenticate");
    }

    #[test]
    fn snake_case_message_id() {
        assert_eq!(to_snake_case("messageId"), "message_id");
    }

    // -- to_pascal_case --

    #[test]
    fn pascal_case_simple() {
        assert_eq!(to_pascal_case("set_protocol"), "SetProtocol");
    }

    #[test]
    fn pascal_case_single_word() {
        assert_eq!(to_pascal_case("login"), "Login");
    }

    #[test]
    fn pascal_case_multiple_words() {
        assert_eq!(
            to_pascal_case("login_plugin_response"),
            "LoginPluginResponse"
        );
    }

    #[test]
    fn pascal_case_already_capitalized() {
        assert_eq!(to_pascal_case("Login"), "Login");
    }

    #[test]
    fn pascal_case_with_numbers() {
        assert_eq!(
            to_pascal_case("legacy_server_list_ping"),
            "LegacyServerListPing"
        );
    }

    // -- short_variant_name --

    #[test]
    fn short_variant_strips_direction_and_state() {
        assert_eq!(
            short_variant_name("ServerboundLoginEncryptionBegin", "Login"),
            "EncryptionBegin"
        );
    }

    #[test]
    fn short_variant_clientbound() {
        assert_eq!(
            short_variant_name("ClientboundStatusServerInfo", "Status"),
            "ServerInfo"
        );
    }

    #[test]
    fn short_variant_no_prefix() {
        assert_eq!(short_variant_name("SomePacket", "Login"), "SomePacket");
    }

    // -- map_type --

    #[test]
    fn map_varint() {
        let (ty, attr, _) = map_type(&Value::String("varint".into()), "", "", false);
        assert_eq!(ty, "i32");
        assert_eq!(attr, Some("varint".into()));
    }

    #[test]
    fn map_string() {
        let (ty, attr, _) = map_type(&Value::String("string".into()), "", "", false);
        assert_eq!(ty, "String");
        assert!(attr.is_none());
    }

    #[test]
    fn map_uuid() {
        let (ty, attr, _) = map_type(&Value::String("UUID".into()), "", "", false);
        assert_eq!(ty, "Uuid");
        assert!(attr.is_none());
    }

    #[test]
    fn map_bool() {
        let (ty, attr, _) = map_type(&Value::String("bool".into()), "", "", false);
        assert_eq!(ty, "bool");
        assert!(attr.is_none());
    }

    #[test]
    fn map_rest_buffer_last() {
        let (ty, attr, _) = map_type(&Value::String("restBuffer".into()), "", "", true);
        assert_eq!(ty, "Vec<u8>");
        assert_eq!(attr, Some("rest".into()));
    }

    #[test]
    fn map_rest_buffer_not_last() {
        let (ty, attr, _) = map_type(&Value::String("restBuffer".into()), "", "", false);
        assert_eq!(ty, "Vec<u8>");
        assert!(attr.is_none());
    }

    #[test]
    fn map_buffer_varint() {
        let json: Value = serde_json::from_str(r#"["buffer", {"countType": "varint"}]"#).unwrap();
        let (ty, attr, _) = map_type(&json, "", "", false);
        assert_eq!(ty, "Vec<u8>");
        assert_eq!(attr, Some("length = \"varint\"".into()));
    }

    #[test]
    fn map_option_string() {
        let json: Value = serde_json::from_str(r#"["option", "string"]"#).unwrap();
        let (ty, attr, _) = map_type(&json, "", "", false);
        assert_eq!(ty, "Option<String>");
        assert_eq!(attr, Some("optional".into()));
    }

    #[test]
    fn map_numeric_types() {
        for (json_type, rust_type) in [
            ("u8", "u8"),
            ("u16", "u16"),
            ("i8", "i8"),
            ("i16", "i16"),
            ("i32", "i32"),
            ("i64", "i64"),
            ("f32", "f32"),
            ("f64", "f64"),
        ] {
            let (ty, attr, _) = map_type(&Value::String(json_type.into()), "", "", false);
            assert_eq!(ty, rust_type, "failed for {json_type}");
            assert!(attr.is_none(), "unexpected attr for {json_type}");
        }
    }

    // -- parse_container_fields --

    #[test]
    fn parse_simple_container() {
        let json: Value = serde_json::from_str(
            r#"
            ["container", [
                {"name": "username", "type": "string"},
                {"name": "age", "type": "varint"}
            ]]
        "#,
        )
        .unwrap();

        let (fields, inlines) = parse_container_fields(&json, "TestPacket");
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "username");
        assert_eq!(fields[0].rust_type, "String");
        assert!(fields[0].attribute.is_none());
        assert_eq!(fields[1].name, "age");
        assert_eq!(fields[1].rust_type, "i32");
        assert_eq!(fields[1].attribute, Some("varint".into()));
        assert!(inlines.is_empty());
    }

    #[test]
    fn parse_container_with_rest_buffer() {
        let json: Value = serde_json::from_str(
            r#"
            ["container", [
                {"name": "id", "type": "varint"},
                {"name": "data", "type": "restBuffer"}
            ]]
        "#,
        )
        .unwrap();

        let (fields, _) = parse_container_fields(&json, "TestPacket");
        assert_eq!(fields[1].name, "data");
        assert_eq!(fields[1].rust_type, "Vec<u8>");
        assert_eq!(fields[1].attribute, Some("rest".into()));
    }

    #[test]
    fn parse_container_with_inline_struct() {
        let json: Value = serde_json::from_str(
            r#"
            ["container", [
                {"name": "items", "type": ["array", {
                    "countType": "varint",
                    "type": ["container", [
                        {"name": "name", "type": "string"},
                        {"name": "value", "type": "i32"}
                    ]]
                }]}
            ]]
        "#,
        )
        .unwrap();

        let (fields, inlines) = parse_container_fields(&json, "TestPacket");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].rust_type, "Vec<TestPacketItems>");
        assert_eq!(fields[0].attribute, Some("length = \"varint\"".into()));
        assert_eq!(inlines.len(), 1);
        assert_eq!(inlines[0].name, "TestPacketItems");
        assert_eq!(inlines[0].fields.len(), 2);
    }

    #[test]
    fn parse_empty_container() {
        let json: Value = serde_json::from_str(r#"["container", []]"#).unwrap();
        let (fields, inlines) = parse_container_fields(&json, "TestPacket");
        assert!(fields.is_empty());
        assert!(inlines.is_empty());
    }

    // -- generate_packet_struct --

    #[test]
    fn generate_unit_struct() {
        let packet = PacketDef {
            name: "TestPingStart".into(),
            id: "0x00".into(),
            fields: vec![],
            inline_structs: vec![],
        };
        let code = generate_packet_struct(&packet);
        assert!(code.contains("#[packet(id = 0x00)]"));
        assert!(code.contains("pub struct TestPingStart;"));
    }

    #[test]
    fn generate_struct_with_fields() {
        let packet = PacketDef {
            name: "TestLogin".into(),
            id: "0x01".into(),
            fields: vec![
                FieldDef {
                    name: "username".into(),
                    rust_type: "String".into(),
                    attribute: None,
                },
                FieldDef {
                    name: "protocol_version".into(),
                    rust_type: "i32".into(),
                    attribute: Some("varint".into()),
                },
            ],
            inline_structs: vec![],
        };
        let code = generate_packet_struct(&packet);
        assert!(code.contains("#[packet(id = 0x01)]"));
        assert!(code.contains("pub struct TestLogin"));
        assert!(code.contains("pub username: String"));
        assert!(code.contains("#[field(varint)]"));
        assert!(code.contains("pub protocol_version: i32"));
    }

    #[test]
    fn generate_struct_with_inline() {
        let packet = PacketDef {
            name: "TestSuccess".into(),
            id: "0x02".into(),
            fields: vec![FieldDef {
                name: "items".into(),
                rust_type: "Vec<TestSuccessItems>".into(),
                attribute: Some("length = \"varint\"".into()),
            }],
            inline_structs: vec![InlineStruct {
                name: "TestSuccessItems".into(),
                fields: vec![FieldDef {
                    name: "name".into(),
                    rust_type: "String".into(),
                    attribute: None,
                }],
            }],
        };
        let code = generate_packet_struct(&packet);
        assert!(code.contains("pub struct TestSuccessItems"));
        assert!(code.contains("pub struct TestSuccess"));
    }

    // -- generate_direction_enum --

    #[test]
    fn generate_enum_with_dispatch() {
        let packets = vec![
            PacketDef {
                name: "ServerboundTestPing".into(),
                id: "0x00".into(),
                fields: vec![],
                inline_structs: vec![],
            },
            PacketDef {
                name: "ServerboundTestStart".into(),
                id: "0x01".into(),
                fields: vec![],
                inline_structs: vec![],
            },
        ];
        let code = generate_direction_enum("ServerboundTestPacket", &packets, "test", "Test");
        assert!(code.contains("pub enum ServerboundTestPacket"));
        assert!(code.contains("Ping(ServerboundTestPing)"));
        assert!(code.contains("Start(ServerboundTestStart)"));
        assert!(code.contains("decode_by_id"));
        assert!(code.contains("ServerboundTestPing::PACKET_ID"));
        assert!(code.contains("state: \"test\""));
    }

    // -- generate_state_module --

    #[test]
    fn generate_module_from_mock_json() {
        let json: Value = serde_json::from_str(r#"{
            "toServer": {
                "types": {
                    "packet": ["container", [
                        {"name": "name", "type": ["mapper", {"type": "varint", "mappings": {"0x00": "ping_start", "0x01": "ping"}}]},
                        {"name": "params", "type": ["switch", {"compareTo": "name", "fields": {"ping_start": "packet_ping_start", "ping": "packet_ping"}}]}
                    ]],
                    "packet_ping_start": ["container", []],
                    "packet_ping": ["container", [{"name": "time", "type": "i64"}]]
                }
            },
            "toClient": {
                "types": {
                    "packet": ["container", [
                        {"name": "name", "type": ["mapper", {"type": "varint", "mappings": {"0x00": "response"}}]},
                        {"name": "params", "type": ["switch", {"compareTo": "name", "fields": {"response": "packet_response"}}]}
                    ]],
                    "packet_response": ["container", [{"name": "data", "type": "string"}]]
                }
            }
        }"#).unwrap();

        let code = generate_state_module(&json, "test");
        assert!(code.contains("Test state packet definitions"));
        assert!(code.contains("ServerboundTestPingStart"));
        assert!(code.contains("ServerboundTestPing"));
        assert!(code.contains("ClientboundTestResponse"));
        assert!(code.contains("ServerboundTestPacket"));
        assert!(code.contains("ClientboundTestPacket"));
        assert!(code.contains("decode_by_id"));
        assert!(code.contains("#[cfg(test)]"));
        assert!(code.contains("_roundtrip"));
    }

    // -- generate_tests --

    #[test]
    fn generate_tests_with_roundtrips() {
        let serverbound = vec![PacketDef {
            name: "ServerboundTestPing".into(),
            id: "0x00".into(),
            fields: vec![FieldDef {
                name: "time".into(),
                rust_type: "i64".into(),
                attribute: None,
            }],
            inline_structs: vec![],
        }];
        let clientbound = vec![PacketDef {
            name: "ClientboundTestPong".into(),
            id: "0x00".into(),
            fields: vec![],
            inline_structs: vec![],
        }];
        let code = generate_tests(&serverbound, &clientbound, "Test", "test");
        assert!(code.contains("serverbound_test_ping_roundtrip"));
        assert!(code.contains("clientbound_test_pong_roundtrip"));
        assert!(code.contains("ServerboundTestPing::default()"));
        assert!(code.contains("ClientboundTestPong;")); // unit struct, no ::default()
        assert!(code.contains("unknown_serverbound_id"));
        assert!(code.contains("unknown_clientbound_id"));
    }

    // -- parse_direction --

    #[test]
    fn parse_direction_skips_common_packets() {
        let json: Value = serde_json::from_str(r#"{
            "toServer": {
                "types": {
                    "packet": ["container", [
                        {"name": "name", "type": ["mapper", {"type": "varint", "mappings": {"0x00": "login_start", "0x01": "cookie_response"}}]},
                        {"name": "params", "type": ["switch", {"compareTo": "name", "fields": {"login_start": "packet_login_start", "cookie_response": "packet_common_cookie_response"}}]}
                    ]],
                    "packet_login_start": ["container", [{"name": "username", "type": "string"}]]
                }
            }
        }"#).unwrap();

        let packets = parse_direction(&json, "toServer", "Serverbound", "Login");
        // Should only have login_start, cookie_response should be skipped
        assert_eq!(packets.len(), 1);
        assert!(packets[0].name.contains("LoginStart"));
    }

    #[test]
    fn parse_direction_empty() {
        let json: Value = serde_json::from_str(r#"{}"#).unwrap();
        let packets = parse_direction(&json, "toServer", "Serverbound", "Login");
        assert!(packets.is_empty());
    }

    // -- map_type edge cases --

    #[test]
    fn map_array_of_primitives() {
        let json: Value =
            serde_json::from_str(r#"["array", {"countType": "varint", "type": "i32"}]"#).unwrap();
        let (ty, attr, _) = map_type(&json, "", "", false);
        assert_eq!(ty, "Vec<i32>");
        assert_eq!(attr, Some("length = \"varint\"".into()));
    }

    #[test]
    fn map_varlong() {
        let (ty, attr, _) = map_type(&Value::String("varlong".into()), "", "", false);
        assert_eq!(ty, "i64");
        assert_eq!(attr, Some("varlong".into()));
    }

    #[test]
    fn map_unknown_type_falls_back() {
        let (ty, _, _) = map_type(&Value::String("unknownType".into()), "", "", false);
        assert_eq!(ty, "Vec<u8>");
    }

    // -- New type mappings --

    #[test]
    fn map_position() {
        let (ty, attr, _) = map_type(&Value::String("position".into()), "", "", false);
        assert_eq!(ty, "Position");
        assert!(attr.is_none());
    }

    #[test]
    fn map_anonymous_nbt() {
        let (ty, attr, _) = map_type(&Value::String("anonymousNbt".into()), "", "", false);
        assert_eq!(ty, "NbtCompound");
        assert!(attr.is_none());
    }

    #[test]
    fn map_anon_optional_nbt() {
        let (ty, attr, _) = map_type(&Value::String("anonOptionalNbt".into()), "", "", false);
        assert_eq!(ty, "Option<NbtCompound>");
        assert_eq!(attr, Some("optional".into()));
    }

    #[test]
    fn map_byte_array_type() {
        let (ty, attr, _) = map_type(&Value::String("ByteArray".into()), "", "", false);
        assert_eq!(ty, "Vec<u8>");
        assert_eq!(attr, Some("length = \"varint\"".into()));
    }

    #[test]
    fn map_container_id() {
        let (ty, attr, _) = map_type(&Value::String("ContainerID".into()), "", "", false);
        assert_eq!(ty, "i32");
        assert_eq!(attr, Some("varint".into()));
    }

    #[test]
    fn map_slot() {
        let (ty, attr, _) = map_type(&Value::String("Slot".into()), "", "", false);
        assert_eq!(ty, "Slot");
        assert!(attr.is_none());
    }

    #[test]
    fn map_vec2f() {
        let (ty, _, _) = map_type(&Value::String("vec2f".into()), "", "", false);
        assert_eq!(ty, "Vec2f");
    }

    #[test]
    fn map_vec3f() {
        let (ty, _, _) = map_type(&Value::String("vec3f".into()), "", "", false);
        assert_eq!(ty, "Vec3f");
    }

    #[test]
    fn map_vec3f64() {
        let (ty, _, _) = map_type(&Value::String("vec3f64".into()), "", "", false);
        assert_eq!(ty, "Vec3f64");
    }

    #[test]
    fn map_vec3i16() {
        let (ty, _, _) = map_type(&Value::String("vec3i16".into()), "", "", false);
        assert_eq!(ty, "Vec3i16");
    }

    #[test]
    fn map_sound_source() {
        let (ty, attr, _) = map_type(&Value::String("soundSource".into()), "", "", false);
        assert_eq!(ty, "i32");
        assert_eq!(attr, Some("varint".into()));
    }

    #[test]
    fn map_packed_chunk_pos() {
        let (ty, _, _) = map_type(&Value::String("packedChunkPos".into()), "", "", false);
        assert_eq!(ty, "i64");
    }

    #[test]
    fn map_optvarint() {
        let (ty, attr, _) = map_type(&Value::String("optvarint".into()), "", "", false);
        assert_eq!(ty, "i32");
        assert_eq!(attr, Some("varint".into()));
    }

    // -- Play category --

    #[test]
    fn play_category_entity() {
        assert_eq!(play_category("spawn_entity"), "entity");
        assert_eq!(play_category("entity_metadata"), "entity");
        assert_eq!(play_category("use_entity"), "entity");
    }

    #[test]
    fn play_category_world() {
        assert_eq!(play_category("block_change"), "world");
        assert_eq!(play_category("map_chunk"), "world");
        assert_eq!(play_category("explosion"), "world");
    }

    #[test]
    fn play_category_player() {
        assert_eq!(play_category("position"), "player");
        assert_eq!(play_category("abilities"), "player");
        assert_eq!(play_category("respawn"), "player");
    }

    #[test]
    fn play_category_inventory() {
        assert_eq!(play_category("window_click"), "inventory");
        assert_eq!(play_category("set_slot"), "inventory");
        assert_eq!(play_category("trade_list"), "inventory");
    }

    #[test]
    fn play_category_chat() {
        assert_eq!(play_category("chat_message"), "chat");
        assert_eq!(play_category("system_chat"), "chat");
        assert_eq!(play_category("boss_bar"), "chat");
    }

    #[test]
    fn play_category_misc_fallback() {
        assert_eq!(play_category("keep_alive"), "misc");
        assert_eq!(play_category("unknown_packet_xyz"), "misc");
    }

    // -- extract_play_short_name --

    #[test]
    fn extract_serverbound_name() {
        assert_eq!(
            extract_play_short_name("ServerboundPlayPosition"),
            "position"
        );
    }

    #[test]
    fn extract_clientbound_name() {
        assert_eq!(
            extract_play_short_name("ClientboundPlayEntityMetadata"),
            "entity_metadata"
        );
    }

    #[test]
    fn extract_compound_name() {
        assert_eq!(
            extract_play_short_name("ServerboundPlayChatCommandSigned"),
            "chat_command_signed"
        );
    }

    // -- generate_category_file --

    #[test]
    fn generate_category_produces_valid_output() {
        let packet = PacketDef {
            name: "ServerboundPlayTest".into(),
            id: "0x00".into(),
            fields: vec![FieldDef {
                name: "value".into(),
                rust_type: "i32".into(),
                attribute: None,
            }],
            inline_structs: vec![],
        };
        let refs = vec![&packet];
        let code = generate_category_file("test", &refs, &[], "Play");
        assert!(code.contains("Play state — test packets"));
        assert!(code.contains("pub struct ServerboundPlayTest"));
        assert!(code.contains("_roundtrip"));
    }

    // -- generate_play_mod --

    #[test]
    fn generate_play_mod_has_enums() {
        let sb = vec![PacketDef {
            name: "ServerboundPlayPing".into(),
            id: "0x00".into(),
            fields: vec![],
            inline_structs: vec![],
        }];
        let cb = vec![PacketDef {
            name: "ClientboundPlayPong".into(),
            id: "0x00".into(),
            fields: vec![],
            inline_structs: vec![],
        }];
        let sb_refs: Vec<&PacketDef> = sb.iter().collect();
        let cb_refs: Vec<&PacketDef> = cb.iter().collect();
        let mut categories = BTreeMap::new();
        categories.insert("misc", (sb_refs, cb_refs));

        let code = generate_play_mod(&categories, &sb, &cb);
        assert!(code.contains("pub mod misc;"));
        assert!(code.contains("ServerboundPlayPacket"));
        assert!(code.contains("ClientboundPlayPacket"));
        assert!(code.contains("decode_by_id"));
    }

    // -- option wrapping inline --

    #[test]
    fn map_option_with_inline_falls_back() {
        let json: Value = serde_json::from_str(
            r#"["option", ["array", {
            "countType": "varint",
            "type": ["container", [{"name": "x", "type": "i32"}]]
        }]]"#,
        )
        .unwrap();
        let (ty, attr, inline) = map_type(&json, "Test", "field", false);
        assert_eq!(ty, "Option<Vec<u8>>");
        assert_eq!(attr, Some("optional".into()));
        assert!(inline.is_none());
    }
}
