//! Basalt public plugin API.
//!
//! This crate is the **single dependency** for all Basalt plugins.
//! Types are organized into focused modules:
//!
//! - [`prelude`] — essentials for every plugin (glob import this)
//! - [`components`] — ECS component types (Position, Velocity, etc.)
//! - [`system`] — system registration (SystemContext, Phase, etc.)
//! - [`command`] — command argument types (Arg, CommandArgs, etc.)
//! - [`types`] — primitive Minecraft types (Uuid, Slot, TextComponent, etc.)
//! - [`world`] — block states, collision, block entities

pub mod broadcast;
pub mod budget;
pub mod components;
pub mod container;
pub mod context;
pub mod events;
pub mod gamemode;
pub mod logger;
pub mod player;
pub mod plugin;
pub mod recipes;
pub mod system;
#[cfg(any(feature = "testing", test))]
pub mod testing;

/// Command argument types, parsing, validation, and dispatch.
pub mod command;

/// Primitive Minecraft types.
pub mod types {
    pub use basalt_types::{NamedColor, Slot, TextColor, TextComponent, Uuid};
}

/// World access: block states, collision, block entities, chunk storage.
pub mod world;

/// Wire-level protocol packet definitions, exposed for plugins that
/// need raw packet inspection (anti-cheat, telemetry, packet logging).
///
/// Most plugins should listen to domain events
/// ([`events::BlockBrokenEvent`], [`events::PlayerMovedEvent`], …)
/// rather than reaching into packet structs directly. The packets
/// module is here for the cases where the wire-level shape matters —
/// e.g. inspecting [`events::RawPacketEvent::packet`].
///
/// Available only when the `raw-packets` feature is enabled.
#[cfg(feature = "raw-packets")]
pub use basalt_mc_protocol::packets;

// Top-level re-exports for non-prelude usage.
pub use context::Response;
pub use events::{Event, EventBus, Stage};
pub use plugin::{CommandEntry, Plugin, PluginMetadata, PluginRegistrar};

/// Prelude module — import this in every plugin.
///
/// Contains only what 90%+ of plugins need: registration types,
/// context traits, events, and stage. Specialized types live in
/// their respective modules.
///
/// ```ignore
/// use basalt_api::prelude::*;
/// ```
pub mod prelude {
    // Plugin registration
    pub use crate::context::Response;
    pub use crate::plugin::{Plugin, PluginMetadata, PluginRegistrar};

    // Context traits
    pub use crate::broadcast::BroadcastMessage;
    pub use crate::context::{
        ChatContext, ContainerContext, Context, EntityContext, PlayerContext, RecipeContext,
        UnlockReason, WorldContext,
    };
    pub use crate::gamemode::Gamemode;

    // Event system
    pub use crate::events::{Event, Stage};

    // Container types
    pub use crate::container::{Container, ContainerBacking, ContainerBuilder, InventoryType};

    // All event types
    pub use crate::events::{
        BlockBrokenEvent, BlockEntityCreatedEvent, BlockEntityDestroyedEvent, BlockEntityKind,
        BlockEntityModifiedEvent, BlockPlacedEvent, ChatMessageEvent, CloseReason, CommandEvent,
        ContainerClickEvent, ContainerClickType, ContainerClosedEvent, ContainerDragEvent,
        ContainerOpenRequestEvent, ContainerOpenedEvent, ContainerSlotChangedEvent,
        CraftingCraftedEvent, CraftingGridChangedEvent, CraftingPreCraftEvent,
        CraftingRecipeClearedEvent, CraftingRecipeMatchedEvent, CraftingShiftClickBatchEvent,
        DragType, PlayerInteractEvent, PlayerJoinedEvent, PlayerLeftEvent, PlayerMovedEvent,
        RecipeBookFillRequestEvent, RecipeBookFilledEvent, RecipeLockedEvent, RecipeRegisterEvent,
        RecipeRegisteredEvent, RecipeUnlockedEvent, RecipeUnregisteredEvent, WindowSlotKind,
    };

    // Recipe types referenced by registry-lifecycle events.
    pub use crate::recipes::{OwnedShapedRecipe, OwnedShapelessRecipe, Recipe, RecipeId};
}
