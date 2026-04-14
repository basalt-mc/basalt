//! Server configuration loaded from `basalt.toml`.
//!
//! The config controls the bind address, world settings (seed, storage
//! mode), and which plugins are enabled. Missing fields use sensible
//! defaults — a missing `basalt.toml` runs a full game server.

use std::path::Path;

use serde::Deserialize;

/// Top-level server configuration.
///
/// Loaded from `basalt.toml` at startup. All fields have defaults
/// so a missing or partial config file works out of the box.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Network and server identity settings.
    pub server: ServerSection,
    /// World generation and storage settings.
    pub world: WorldSection,
    /// Plugin enable/disable flags.
    pub plugins: PluginsSection,
}

/// Network settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerSection {
    /// Address to bind the TCP listener to.
    pub bind: String,
}

/// World generation and storage settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WorldSection {
    /// Terrain generation seed.
    pub seed: u32,
    /// Storage mode: how chunks are persisted to disk.
    pub storage: StorageMode,
}

/// How chunks are persisted to disk.
///
/// - `none` — no disk access, chunks exist only in memory
/// - `read-only` — load pre-built maps from disk, never write
/// - `read-write` — load from disk and save modifications
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StorageMode {
    /// No disk access. Chunks generated in memory only.
    None,
    /// Load chunks from disk, never write back.
    ReadOnly,
    /// Load from disk and save modifications.
    #[default]
    ReadWrite,
}

/// Plugin enable/disable flags.
///
/// Each flag controls whether the corresponding plugin is registered
/// on the event bus at startup. Disabled plugins have zero overhead.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PluginsSection {
    /// Chat messages and slash commands.
    pub chat: bool,
    /// Block breaking and placing.
    pub block: bool,
    /// Chunk streaming on movement.
    pub world: bool,
    /// Player join/leave broadcasts.
    pub lifecycle: bool,
    /// Player movement broadcasts.
    pub movement: bool,
}

impl Default for ServerSection {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:25565".into(),
        }
    }
}

impl Default for WorldSection {
    fn default() -> Self {
        Self {
            seed: 42,
            storage: StorageMode::ReadWrite,
        }
    }
}

impl Default for PluginsSection {
    fn default() -> Self {
        Self {
            chat: true,
            block: true,
            world: true,
            lifecycle: true,
            movement: true,
        }
    }
}

impl ServerConfig {
    /// Loads the config from `basalt.toml` in the current directory.
    ///
    /// Returns the default config if the file doesn't exist.
    /// Panics if the file exists but contains invalid TOML.
    pub fn load() -> Self {
        Self::load_from(Path::new("basalt.toml"))
    }

    /// Loads the config from the given path.
    ///
    /// Returns the default config if the file doesn't exist.
    pub fn load_from(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
                panic!("Failed to parse {}: {e}", path.display());
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                println!("[config] No basalt.toml found, using defaults");
                Self::default()
            }
            Err(e) => {
                panic!("Failed to read {}: {e}", path.display());
            }
        }
    }

    /// Creates the world based on the config settings.
    ///
    /// - `none` → in-memory world, no disk
    /// - `read-only` → loads from `world/`, no writes
    /// - `read-write` → loads from `world/`, writes back
    pub fn create_world(&self) -> basalt_world::World {
        match self.world.storage {
            StorageMode::None => {
                println!("[world] Memory-only (no persistence)");
                basalt_world::World::new_memory(self.world.seed)
            }
            StorageMode::ReadOnly | StorageMode::ReadWrite => {
                println!(
                    "[world] Storage: {:?}, seed: {}, dir: world/",
                    self.world.storage, self.world.seed
                );
                basalt_world::World::new(self.world.seed, "world")
            }
        }
    }

    /// Returns the list of plugins to register based on the config.
    pub fn create_plugins(&self) -> Vec<Box<dyn basalt_api::Plugin>> {
        let mut plugins: Vec<Box<dyn basalt_api::Plugin>> = Vec::new();

        if self.plugins.lifecycle {
            plugins.push(Box::new(basalt_plugin_lifecycle::LifecyclePlugin));
        }
        if self.plugins.chat {
            plugins.push(Box::new(basalt_plugin_chat::ChatPlugin));
        }
        if self.plugins.movement {
            plugins.push(Box::new(basalt_plugin_movement::MovementPlugin));
        }
        if self.plugins.world {
            plugins.push(Box::new(basalt_plugin_world::WorldPlugin));
        }
        if self.plugins.block {
            plugins.push(Box::new(basalt_plugin_block::BlockPlugin));
        }
        // StoragePlugin is only active in read-write mode
        if self.plugins.block && self.world.storage == StorageMode::ReadWrite {
            plugins.push(Box::new(basalt_plugin_storage::StoragePlugin));
        }

        plugins
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = ServerConfig::default();
        assert_eq!(config.server.bind, "0.0.0.0:25565");
        assert_eq!(config.world.seed, 42);
        assert_eq!(config.world.storage, StorageMode::ReadWrite);
        assert!(config.plugins.chat);
        assert!(config.plugins.block);
        assert!(config.plugins.world);
        assert!(config.plugins.lifecycle);
        assert!(config.plugins.movement);
    }

    #[test]
    fn parse_full_config() {
        let toml = r#"
[server]
bind = "127.0.0.1:25566"

[world]
seed = 123
storage = "read-only"

[plugins]
chat = true
block = false
world = true
lifecycle = true
movement = false
"#;
        let config: ServerConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.server.bind, "127.0.0.1:25566");
        assert_eq!(config.world.seed, 123);
        assert_eq!(config.world.storage, StorageMode::ReadOnly);
        assert!(config.plugins.chat);
        assert!(!config.plugins.block);
        assert!(config.plugins.world);
        assert!(config.plugins.lifecycle);
        assert!(!config.plugins.movement);
    }

    #[test]
    fn parse_partial_config() {
        let toml = r#"
[world]
seed = 99
"#;
        let config: ServerConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.server.bind, "0.0.0.0:25565"); // default
        assert_eq!(config.world.seed, 99);
        assert_eq!(config.world.storage, StorageMode::ReadWrite); // default
        assert!(config.plugins.chat); // default
    }

    #[test]
    fn parse_empty_config() {
        let config: ServerConfig = toml::from_str("").unwrap();
        assert_eq!(config.server.bind, "0.0.0.0:25565");
        assert_eq!(config.world.seed, 42);
    }

    #[test]
    fn storage_none_mode() {
        let toml = r#"
[world]
storage = "none"
"#;
        let config: ServerConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.world.storage, StorageMode::None);
    }

    #[test]
    fn create_plugins_all_enabled() {
        let config = ServerConfig::default();
        let plugins = config.create_plugins();
        // 6 plugins: lifecycle, chat, movement, world, block, storage
        assert_eq!(plugins.len(), 6);
    }

    #[test]
    fn create_plugins_read_only_no_storage() {
        let mut config = ServerConfig::default();
        config.world.storage = StorageMode::ReadOnly;
        let plugins = config.create_plugins();
        // 5 plugins: no StoragePlugin
        assert_eq!(plugins.len(), 5);
        assert!(plugins.iter().all(|p| p.metadata().name != "storage"));
    }

    #[test]
    fn create_plugins_none_disabled() {
        let mut config = ServerConfig::default();
        config.plugins.chat = false;
        config.plugins.block = false;
        config.plugins.world = false;
        config.plugins.lifecycle = false;
        config.plugins.movement = false;
        let plugins = config.create_plugins();
        assert!(plugins.is_empty());
    }

    #[test]
    fn load_missing_file_returns_default() {
        let config = ServerConfig::load_from(Path::new("nonexistent.toml"));
        assert_eq!(config.server.bind, "0.0.0.0:25565");
    }
}
