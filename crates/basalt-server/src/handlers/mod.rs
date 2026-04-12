//! Built-in plugin handlers for the event bus.
//!
//! Each handler is a struct with a `register(&mut EventBus)` method
//! that adds its event handlers to the bus. Handlers are registered
//! at server startup based on configuration — disabled plugins have
//! zero overhead since their handlers are never added.

mod block;
mod chat;
mod lifecycle;
mod movement;
mod storage;
mod world;

pub use block::BlockInteractionHandler;
pub use chat::ChatHandler;
pub use lifecycle::LifecycleHandler;
pub use movement::PlayerInputHandler;
pub use storage::StorageHandler;
pub use world::WorldHandler;

use basalt_events::EventBus;

/// Registers all built-in plugin handlers on the event bus.
///
/// In the future, this will be config-driven — each plugin is only
/// registered if its feature flag is enabled. For now, all built-in
/// handlers are always active.
pub fn register_all(bus: &mut EventBus) {
    LifecycleHandler::register(bus);
    ChatHandler::register(bus);
    PlayerInputHandler::register(bus);
    WorldHandler::register(bus);
    BlockInteractionHandler::register(bus);
    StorageHandler::register(bus);
}
