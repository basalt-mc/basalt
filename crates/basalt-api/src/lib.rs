//! Basalt public plugin API.
//!
//! This crate defines the complete public API for Basalt server
//! plugins. Both built-in plugins and external plugins depend on
//! this crate — there is no separate internal API.
//!
//! # Writing a plugin
//!
//! 1. Implement the [`Plugin`] trait with metadata and event registration
//! 2. Use [`EventRegistrar`] to subscribe to events at specific stages
//! 3. Use [`ServerContext`] methods in handlers to interact with the server
//!
//! ```ignore
//! use basalt_api::prelude::*;
//!
//! pub struct MyPlugin;
//!
//! impl Plugin for MyPlugin {
//!     fn metadata(&self) -> PluginMetadata {
//!         PluginMetadata {
//!             name: "my-plugin",
//!             version: "0.1.0",
//!             author: Some("Me"),
//!             dependencies: &[],
//!         }
//!     }
//!
//!     fn on_enable(&self, registrar: &mut EventRegistrar) {
//!         registrar.on::<PlayerJoinedEvent>(Stage::Post, 0, |_event, ctx| {
//!             ctx.send_message("Welcome!");
//!         });
//!     }
//! }
//! ```

pub mod broadcast;
pub mod context;
pub mod events;
pub mod logger;
pub mod plugin;

// Re-export core types at crate root for convenience.
pub use broadcast::{BroadcastMessage, PlayerSnapshot, ProfileProperty};
pub use context::{Response, ServerContext};
pub use plugin::{EventRegistrar, Plugin, PluginMetadata};

// Re-export basalt-events types that plugins need.
pub use basalt_events::{Event, EventBus, Stage};

/// Prelude module for convenient glob imports.
///
/// ```ignore
/// use basalt_api::prelude::*;
/// ```
pub mod prelude {
    pub use crate::broadcast::{BroadcastMessage, PlayerSnapshot};
    pub use crate::context::ServerContext;
    pub use crate::events::{
        BlockBrokenEvent, BlockPlacedEvent, ChatMessageEvent, CommandEvent, PlayerJoinedEvent,
        PlayerLeftEvent, PlayerMovedEvent,
    };
    pub use crate::plugin::{EventRegistrar, Plugin, PluginMetadata};
    pub use basalt_events::Stage;
}
