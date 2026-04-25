//! Game events dispatched through the event bus.
//!
//! Events are grouped by domain:
//! - [`block`] — block breaking, placing, interaction
//! - [`container`] — container open/close, clicks, drags, block entities
//! - [`player`] — movement, join, leave
//! - [`chat`] — chat messages and commands
//!
//! Use the macros ([`game_cancellable_event!`], [`game_event!`],
//! [`instant_cancellable_event!`], [`instant_event!`]) to implement
//! the [`Event`](basalt_events::Event) trait for custom event types.

mod block;
mod chat;
mod container;
mod crafting;
mod player;

pub use block::{BlockBrokenEvent, BlockPlacedEvent, PlayerInteractEvent};
pub use chat::{ChatMessageEvent, CommandEvent};
pub use container::*;
pub use crafting::{
    CraftingCraftedEvent, CraftingGridChangedEvent, CraftingPreCraftEvent,
    CraftingRecipeClearedEvent, CraftingRecipeMatchedEvent, CraftingShiftClickBatchEvent,
};
pub use player::{PlayerJoinedEvent, PlayerLeftEvent, PlayerMovedEvent};

/// Implements [`Event`](basalt_events::Event) and
/// [`EventRouting`](basalt_events::EventRouting) for a non-cancellable
/// event dispatched on the **instant** loop's bus.
#[macro_export]
macro_rules! instant_event {
    ($name:ident) => {
        impl basalt_events::Event for $name {
            fn is_cancelled(&self) -> bool {
                false
            }
            fn cancel(&mut self) {}
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
            fn bus_kind(&self) -> basalt_events::BusKind {
                basalt_events::BusKind::Instant
            }
        }
        impl basalt_events::EventRouting for $name {
            const BUS: basalt_events::BusKind = basalt_events::BusKind::Instant;
        }
    };
}

/// Implements [`Event`](basalt_events::Event) and
/// [`EventRouting`](basalt_events::EventRouting) for a cancellable
/// event dispatched on the **instant** loop's bus.
///
/// The struct must have a `cancelled: bool` field.
#[macro_export]
macro_rules! instant_cancellable_event {
    ($name:ident) => {
        impl basalt_events::Event for $name {
            fn is_cancelled(&self) -> bool {
                self.cancelled
            }
            fn cancel(&mut self) {
                self.cancelled = true;
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
            fn bus_kind(&self) -> basalt_events::BusKind {
                basalt_events::BusKind::Instant
            }
        }
        impl basalt_events::EventRouting for $name {
            const BUS: basalt_events::BusKind = basalt_events::BusKind::Instant;
        }
    };
}

/// Implements [`Event`](basalt_events::Event) and
/// [`EventRouting`](basalt_events::EventRouting) for a non-cancellable
/// event dispatched on the **game** loop's bus.
#[macro_export]
macro_rules! game_event {
    ($name:ident) => {
        impl basalt_events::Event for $name {
            fn is_cancelled(&self) -> bool {
                false
            }
            fn cancel(&mut self) {}
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
            fn bus_kind(&self) -> basalt_events::BusKind {
                basalt_events::BusKind::Game
            }
        }
        impl basalt_events::EventRouting for $name {
            const BUS: basalt_events::BusKind = basalt_events::BusKind::Game;
        }
    };
}

/// Implements [`Event`](basalt_events::Event) and
/// [`EventRouting`](basalt_events::EventRouting) for a cancellable
/// event dispatched on the **game** loop's bus.
///
/// The struct must have a `cancelled: bool` field.
#[macro_export]
macro_rules! game_cancellable_event {
    ($name:ident) => {
        impl basalt_events::Event for $name {
            fn is_cancelled(&self) -> bool {
                self.cancelled
            }
            fn cancel(&mut self) {
                self.cancelled = true;
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
            fn bus_kind(&self) -> basalt_events::BusKind {
                basalt_events::BusKind::Game
            }
        }
        impl basalt_events::EventRouting for $name {
            const BUS: basalt_events::BusKind = basalt_events::BusKind::Game;
        }
    };
}
