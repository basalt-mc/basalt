//! Play state packet generation with category-based file splitting.
//!
//! The Play state has ~180 packets — too many for a single file.
//! This module splits them into category sub-files (entity, world,
//! player, inventory, chat, misc) and generates a `play/mod.rs`
//! that re-exports everything and provides direction dispatch enums.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::codegen::{generate_direction_enum, generate_imports, generate_packet_struct};
use crate::helpers::{format_file, to_snake_case};
use crate::parser::parse_direction;
use crate::types::PacketDef;

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
///
/// Returns the matching category from `PLAY_CATEGORIES`, or "misc"
/// as a fallback for packets that don't fit any predefined category.
fn play_category(packet_short_name: &str) -> &'static str {
    for &(category, names) in PLAY_CATEGORIES {
        if names.contains(&packet_short_name) {
            return category;
        }
    }
    "misc"
}

/// Extracts the short packet name from a full struct name.
///
/// Strips the direction prefix and "Play" state name, then converts
/// to snake_case. For example, "ClientboundPlayEntityMetadata"
/// becomes "entity_metadata".
fn extract_play_short_name(full_name: &str) -> String {
    let without_dir = full_name
        .strip_prefix("Serverbound")
        .or_else(|| full_name.strip_prefix("Clientbound"))
        .unwrap_or(full_name);
    let without_state = without_dir.strip_prefix("Play").unwrap_or(without_dir);
    to_snake_case(without_state)
}

/// Generates the play state as a directory with category sub-files.
///
/// Creates `play/` directory with one `.rs` file per category
/// (entity, world, player, inventory, chat, misc) plus a `mod.rs`
/// that re-exports all packets and defines the direction enums.
pub(crate) fn generate_play_split(
    state: &Value,
    workspace_root: &Path,
    packets_dir: &str,
    global_types: &Value,
) {
    let play_dir = workspace_root.join(packets_dir).join("play");
    fs::create_dir_all(&play_dir)
        .unwrap_or_else(|e| panic!("Failed to create {}: {e}", play_dir.display()));

    let pascal_state = "Play";
    let serverbound = parse_direction(state, "toServer", "Serverbound", pascal_state, global_types);
    let clientbound = parse_direction(state, "toClient", "Clientbound", pascal_state, global_types);

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
        let code = generate_category_file(category, sb, cb);
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

/// Generates a category sub-file with packet structs.
///
/// Each category file contains the struct definitions for all packets
/// in that category (both serverbound and clientbound), with the
/// appropriate derive macros and imports.
fn generate_category_file(
    category: &str,
    serverbound: &[&PacketDef],
    clientbound: &[&PacketDef],
) -> String {
    let mut output = String::new();

    output.push_str(&format!("//! Play state — {category} packets.\n"));
    output.push_str("//!\n");
    output.push_str("//! Auto-generated by `cargo xt codegen` from minecraft-data.\n");
    output.push_str("//! Do not edit manually — changes will be overwritten.\n\n");

    let all_packets: Vec<&PacketDef> = serverbound
        .iter()
        .chain(clientbound.iter())
        .copied()
        .collect();

    output.push_str(&generate_imports(&all_packets));
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

    output
}

/// Generates the `play/mod.rs` that re-exports category modules and
/// defines the direction enums with `decode_by_id` spanning all
/// categories.
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
            for switch_enum in &packet.switch_enums {
                out.push_str(&format!("pub use {category}::{};\n", switch_enum.name));
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

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FieldDef, PacketDef};

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
            switch_enums: vec![],
        };
        let refs = vec![&packet];
        let code = generate_category_file("test", &refs, &[]);
        assert!(code.contains("Play state — test packets"));
        assert!(code.contains("pub struct ServerboundPlayTest"));
    }

    #[test]
    fn generate_play_mod_has_enums() {
        let sb = vec![PacketDef {
            name: "ServerboundPlayPing".into(),
            id: "0x00".into(),
            fields: vec![],
            inline_structs: vec![],
            switch_enums: vec![],
        }];
        let cb = vec![PacketDef {
            name: "ClientboundPlayPong".into(),
            id: "0x00".into(),
            fields: vec![],
            inline_structs: vec![],
            switch_enums: vec![],
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
}
