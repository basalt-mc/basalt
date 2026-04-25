//! Game events and the event bus.
//!
//! This module provides both:
//! - The generic event-bus infrastructure ([`EventBus`], [`Event`],
//!   [`Stage`], [`BusKind`], [`EventRouting`])
//! - Domain event types organized by area: [`block`](self#block-events),
//!   [`chat`](self#chat-events), [`container`](self#container-events),
//!   [`player`](self#player-events), and crafting/recipe events.
//!
//! Events are dispatched through the bus in three stages:
//!
//! 1. **Validate** — read-only checks, can cancel (permissions, anti-cheat)
//! 2. **Process** — state mutation (world changes, inventory updates)
//! 3. **Post** — side effects (broadcasting, persistence, logging)
//!
//! If any Validate handler cancels an event, Process and Post are
//! skipped entirely.
//!
//! Use the macros ([`game_cancellable_event!`], [`game_event!`],
//! [`instant_cancellable_event!`], [`instant_event!`]) to implement
//! the [`Event`] trait for custom event types.

mod block;
mod bus;
mod chat;
mod container;
mod crafting;
mod packet;
mod player;
mod traits;

pub use block::{BlockBrokenEvent, BlockPlacedEvent, PlayerInteractEvent};
pub use bus::EventBus;
pub use chat::{ChatMessageEvent, CommandEvent};
pub use container::*;
pub use crafting::{
    CraftingCraftedEvent, CraftingGridChangedEvent, CraftingPreCraftEvent,
    CraftingRecipeClearedEvent, CraftingRecipeMatchedEvent, CraftingShiftClickBatchEvent,
    RecipeBookFillRequestEvent, RecipeBookFilledEvent, RecipeLockedEvent, RecipeRegisterEvent,
    RecipeRegisteredEvent, RecipeUnlockedEvent, RecipeUnregisteredEvent,
};
pub use packet::RawPacketEvent;
pub use player::{PlayerJoinedEvent, PlayerLeftEvent, PlayerMovedEvent};
pub use traits::{BusKind, Event, EventRouting, Stage};

/// Implements [`Event`] and [`EventRouting`] for a non-cancellable
/// event dispatched on the **instant** loop's bus.
#[macro_export]
macro_rules! instant_event {
    ($name:ident) => {
        impl $crate::events::Event for $name {
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
            fn bus_kind(&self) -> $crate::events::BusKind {
                $crate::events::BusKind::Instant
            }
        }
        impl $crate::events::EventRouting for $name {
            const BUS: $crate::events::BusKind = $crate::events::BusKind::Instant;
        }
    };
}

/// Implements [`Event`] and [`EventRouting`] for a cancellable
/// event dispatched on the **instant** loop's bus.
///
/// The struct must have a `cancelled: bool` field.
#[macro_export]
macro_rules! instant_cancellable_event {
    ($name:ident) => {
        impl $crate::events::Event for $name {
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
            fn bus_kind(&self) -> $crate::events::BusKind {
                $crate::events::BusKind::Instant
            }
        }
        impl $crate::events::EventRouting for $name {
            const BUS: $crate::events::BusKind = $crate::events::BusKind::Instant;
        }
    };
}

/// Implements [`Event`] and [`EventRouting`] for a non-cancellable
/// event dispatched on the **game** loop's bus.
#[macro_export]
macro_rules! game_event {
    ($name:ident) => {
        impl $crate::events::Event for $name {
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
            fn bus_kind(&self) -> $crate::events::BusKind {
                $crate::events::BusKind::Game
            }
        }
        impl $crate::events::EventRouting for $name {
            const BUS: $crate::events::BusKind = $crate::events::BusKind::Game;
        }
    };
}

/// Implements [`Event`] and [`EventRouting`] for a cancellable
/// event dispatched on the **game** loop's bus.
///
/// The struct must have a `cancelled: bool` field.
#[macro_export]
macro_rules! game_cancellable_event {
    ($name:ident) => {
        impl $crate::events::Event for $name {
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
            fn bus_kind(&self) -> $crate::events::BusKind {
                $crate::events::BusKind::Game
            }
        }
        impl $crate::events::EventRouting for $name {
            const BUS: $crate::events::BusKind = $crate::events::BusKind::Game;
        }
    };
}
