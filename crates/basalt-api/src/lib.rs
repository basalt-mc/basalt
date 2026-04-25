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
pub mod container;
pub mod context;
pub mod events;
pub mod logger;
pub mod plugin;
pub mod recipes;

/// ECS component types for system plugins.
///
/// Contains spatial (Position, Velocity), identity (PlayerRef, EntityKind),
/// item (DroppedItem, Lifetime), and inventory components.
pub mod components {
    pub use basalt_core::components::*;
    pub use basalt_core::{Component, EntityId};
}

/// System registration for tick-based plugins.
///
/// System plugins register a runner that executes each tick with
/// access to entities and the world via [`SystemContext`].
pub mod system {
    pub use basalt_core::{
        Phase, SystemAccess, SystemBuilder, SystemContext, SystemContextExt, SystemDescriptor,
        TickBudget,
    };
}

/// Command argument types for command plugins.
pub mod command {
    pub use basalt_command::{Arg, CommandArg, CommandArgs, Validation};
}

/// Primitive Minecraft types.
pub mod types {
    pub use basalt_types::{NamedColor, Slot, TextColor, TextComponent, Uuid};
}

/// World access: block states, collision, block entities, chunk storage.
pub use basalt_world as world;

// Top-level re-exports for non-prelude usage.
pub use basalt_events::{Event, EventBus, Stage};
pub use context::{Response, ServerContext};
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
    pub use crate::context::{Response, ServerContext};
    pub use crate::plugin::{Plugin, PluginMetadata, PluginRegistrar};

    // Context traits
    pub use basalt_core::{
        BroadcastMessage, ChatContext, ContainerContext, Context, EntityContext, Gamemode,
        PlayerContext, WorldContext,
    };

    // Event system
    pub use basalt_events::{Event, Stage};

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
        RecipeRegisterEvent, RecipeRegisteredEvent, RecipeUnregisteredEvent, WindowSlotKind,
    };

    // Recipe types referenced by registry-lifecycle events.
    pub use basalt_recipes::{OwnedShapedRecipe, OwnedShapelessRecipe, Recipe, RecipeId};
}
