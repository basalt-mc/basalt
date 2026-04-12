//! Code generation tool for Minecraft protocol packets.
//!
//! Reads packet definitions from the minecraft-data JSON files and generates
//! Rust source code with `#[packet(id)]` attribute macros and typed fields.
//!
//! Usage: `cargo xt codegen`

mod codegen;
mod helpers;
mod parser;
mod play;
mod types;

use std::fs;

use serde_json::Value;

use codegen::{generate_packets_mod, generate_state_module};
use helpers::{find_workspace_root, format_file};
use play::generate_play_split;

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
        Some("codegen") => run_codegen(),
        _ => {
            eprintln!("Usage: cargo xt codegen");
            std::process::exit(1);
        }
    }
}

/// Runs the code generation pipeline for all configured states.
///
/// Reads `protocol.json` from the minecraft-data submodule, generates
/// Rust source for each protocol state, and writes the output files
/// to the packets directory. Play state is split into category
/// sub-files; all other states produce a single `.rs` file.
fn run_codegen() {
    let workspace_root = find_workspace_root();
    let protocol_path = workspace_root
        .join(MINECRAFT_DATA_PATH)
        .join(VERSION)
        .join("protocol.json");

    println!("Reading protocol data from {}", protocol_path.display());
    let protocol_json = fs::read_to_string(&protocol_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", protocol_path.display()));

    let protocol: Value = serde_json::from_str(&protocol_json).expect("Failed to parse JSON");
    let global_types = &protocol["types"];

    for &(json_key, module_name) in STATES {
        let state_data = &protocol[json_key];
        if state_data.is_null() {
            eprintln!("Warning: state '{json_key}' not found in protocol.json, skipping");
            continue;
        }

        if module_name == "play" {
            generate_play_split(state_data, &workspace_root, PACKETS_DIR, global_types);
        } else {
            let code = generate_state_module(state_data, module_name, global_types);
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
    let mod_code = generate_packets_mod(&workspace_root, PACKETS_DIR);
    fs::write(&mod_path, &mod_code)
        .unwrap_or_else(|e| panic!("Failed to write {}: {e}", mod_path.display()));
    format_file(&mod_path);

    println!("Done.");
}
