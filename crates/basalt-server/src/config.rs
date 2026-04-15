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

/// Network and runtime settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerSection {
    /// Address to bind the TCP listener to.
    pub bind: String,
    /// Log level: trace, debug, info, warn, error.
    pub log_level: LogLevel,
    /// Log format: pretty (human-readable) or json (structured).
    pub log_format: LogFormat,
    /// Performance tuning.
    pub performance: PerformanceSection,
}

/// Performance tuning settings.
///
/// Configured via `[server.performance]` in `basalt.toml`.
///
/// # Example
///
/// ```toml
/// [server.performance]
/// # Max chunks in memory. Each chunk ≈ 192 KB.
/// # 4096 chunks ≈ 768 MB, 8192 chunks ≈ 1.5 GB.
/// chunk_cache_max_entries = 4096
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PerformanceSection {
    /// Maximum number of chunks kept in the memory cache.
    ///
    /// When exceeded, the least recently accessed chunks are evicted.
    /// Dirty chunks (modified since last persist) are saved to disk
    /// before eviction.
    ///
    /// Each chunk uses approximately 192 KB of memory.
    /// Default: 4096 (~768 MB).
    pub chunk_cache_max_entries: usize,
}

/// Log output format.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Human-readable format with colors and aligned fields.
    #[default]
    Pretty,
    /// Structured JSON, one object per line.
    Json,
}

/// Log verbosity level.
///
/// Maps directly to `log::LevelFilter`. Configurable via `basalt.toml`
/// and overridable via the `RUST_LOG` environment variable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Very verbose: keep-alive RTT, chunk counts, unhandled packets.
    Trace,
    /// Protocol flow: packets sent/received, state transitions.
    Debug,
    /// Important events: server start, player join/leave, plugins loaded.
    #[default]
    Info,
    /// Non-critical problems: skin fetch failure, keep-alive mismatch.
    Warn,
    /// Connection errors and fatal issues.
    Error,
}

impl LogLevel {
    /// Converts to `log::LevelFilter` for logger initialization.
    pub fn to_level_filter(self) -> log::LevelFilter {
        match self {
            Self::Trace => log::LevelFilter::Trace,
            Self::Debug => log::LevelFilter::Debug,
            Self::Info => log::LevelFilter::Info,
            Self::Warn => log::LevelFilter::Warn,
            Self::Error => log::LevelFilter::Error,
        }
    }
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
    /// Chat message broadcast.
    pub chat: bool,
    /// Gameplay commands (/tp, /gamemode, /say, /help).
    pub command: bool,
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
            log_level: LogLevel::Info,
            log_format: LogFormat::Pretty,
            performance: PerformanceSection::default(),
        }
    }
}

impl Default for PerformanceSection {
    fn default() -> Self {
        Self {
            chunk_cache_max_entries: 4096,
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
            command: true,
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

    /// Initializes the logger based on the config's log level and format.
    ///
    /// Uses `env_logger` with the configured level as default.
    /// The `RUST_LOG` environment variable overrides the config
    /// if set, allowing runtime adjustment without editing the file.
    ///
    /// Formats:
    /// - `pretty`: `[2026-04-14 10:32:01] INFO  [basalt::server] message`
    /// - `json`: `{"ts":"2026-04-14T10:32:01Z","level":"INFO","target":"basalt::server","msg":"message"}`
    pub fn init_logger(&self) {
        use std::io::Write;

        let format = self.server.log_format;
        env_logger::Builder::new()
            .filter_level(self.server.log_level.to_level_filter())
            .parse_default_env()
            .format(move |buf, record| match format {
                LogFormat::Pretty => {
                    let level = record.level();
                    let target = record.target();
                    writeln!(
                        buf,
                        "{} {level:<5} [{target}] {}",
                        buf.timestamp(),
                        record.args()
                    )
                }
                LogFormat::Json => {
                    writeln!(
                        buf,
                        r#"{{"ts":"{}","level":"{}","target":"{}","msg":"{}"}}"#,
                        buf.timestamp(),
                        record.level(),
                        record.target(),
                        record.args()
                    )
                }
            })
            .init();
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
                log::info!("No basalt.toml found, using defaults");
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
        let max_chunks = self.server.performance.chunk_cache_max_entries;
        let approx_mb = max_chunks * 192 / 1024;
        match self.world.storage {
            StorageMode::None => {
                log::info!(
                    "World: memory-only (no persistence), seed {}, cache {max_chunks} chunks (~{approx_mb} MB)",
                    self.world.seed
                );
                basalt_world::World::new_memory_with_capacity(self.world.seed, max_chunks)
            }
            StorageMode::ReadOnly | StorageMode::ReadWrite => {
                log::info!(
                    "World: {:?} storage, seed {}, dir world/, cache {max_chunks} chunks (~{approx_mb} MB)",
                    self.world.storage,
                    self.world.seed
                );
                basalt_world::World::new_with_capacity(self.world.seed, "world", max_chunks)
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
        if self.plugins.command {
            plugins.push(Box::new(basalt_plugin_command::CommandPlugin));
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
        // 7 plugins: lifecycle, chat, command, movement, world, block, storage
        assert_eq!(plugins.len(), 7);
    }

    #[test]
    fn create_plugins_read_only_no_storage() {
        let mut config = ServerConfig::default();
        config.world.storage = StorageMode::ReadOnly;
        let plugins = config.create_plugins();
        // 6 plugins: no StoragePlugin
        assert_eq!(plugins.len(), 6);
        assert!(plugins.iter().all(|p| p.metadata().name != "storage"));
    }

    #[test]
    fn create_plugins_none_disabled() {
        let mut config = ServerConfig::default();
        config.plugins.chat = false;
        config.plugins.command = false;
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
