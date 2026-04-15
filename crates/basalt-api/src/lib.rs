//! Basalt public plugin API.
//!
//! This crate defines the complete public API for Basalt server
//! plugins. Both built-in plugins and external plugins depend on
//! this crate — there is no separate internal API.

pub mod broadcast;
pub mod context;
pub mod events;
pub mod logger;
pub mod plugin;

// Re-export core types for convenience.
pub use basalt_core::{BroadcastMessage, Context, Gamemode, PlayerSnapshot, ProfileProperty};
pub use context::{Response, ServerContext};
pub use plugin::{CommandEntry, Plugin, PluginMetadata, PluginRegistrar};

// Re-export command types for convenience.
pub use basalt_command::{Arg, CommandArg, CommandArgs, Validation};

// Re-export basalt-events types.
pub use basalt_events::{Event, EventBus, Stage};

/// Prelude module for convenient glob imports.
pub mod prelude {
    pub use basalt_command::{Arg, CommandArgs, Validation};
    pub use basalt_core::{BroadcastMessage, Context, Gamemode, PlayerSnapshot};

    pub use crate::context::ServerContext;
    pub use crate::events::{
        BlockBrokenEvent, BlockPlacedEvent, ChatMessageEvent, CommandEvent, PlayerJoinedEvent,
        PlayerLeftEvent, PlayerMovedEvent,
    };
    pub use crate::plugin::{Plugin, PluginMetadata, PluginRegistrar};
    pub use basalt_events::Stage;
}
