//! Event dispatch context and response queue.
//!
//! The [`ServerContext`] is the concrete implementation of
//! [`Context`](basalt_core::Context) for in-game player contexts.
//! It queues deferred responses that the play loop executes after
//! event dispatch completes.

mod chat;
mod container;
mod entity;
mod player;
mod response;
mod world;

#[cfg(test)]
mod tests;

pub use response::Response;
pub(crate) use response::ResponseQueue;

use std::cell::RefCell;
use std::sync::Arc;

use basalt_core::player::PlayerInfo;
use basalt_core::{
    ChatContext, ContainerContext, Context, EntityContext, PlayerContext, PluginLogger,
    WorldContext,
};

/// Context available to event handlers during dispatch.
///
/// Implements [`Context`] for in-game players. Created per-dispatch
/// on the stack. Internal methods (`new`, `set_plugin_name`,
/// `drain_responses`) are not part of the `Context` trait.
pub struct ServerContext {
    /// Shared world reference for block access and chunk persistence.
    pub(super) world: Arc<basalt_world::World>,
    /// Queue for deferred async responses.
    pub(super) responses: ResponseQueue,
    /// Identity and state of the player who triggered this action.
    pub(super) player: PlayerInfo,
    /// Name of the plugin currently being dispatched.
    pub(super) plugin_name: RefCell<String>,
    /// Registered command list (name, description) for /help.
    pub(super) command_list: RefCell<Vec<(String, String)>>,
}

impl ServerContext {
    /// Creates a new context for a single event dispatch.
    pub fn new(world: Arc<basalt_world::World>, player: PlayerInfo) -> Self {
        Self {
            world,
            responses: ResponseQueue::new(),
            player,
            plugin_name: RefCell::new(String::new()),
            command_list: RefCell::new(Vec::new()),
        }
    }

    /// Sets the registered command list for /help.
    pub fn set_command_list(&self, commands: Vec<(String, String)>) {
        *self.command_list.borrow_mut() = commands;
    }

    /// Sets the plugin name for logger context.
    pub fn set_plugin_name(&self, name: &str) {
        *self.plugin_name.borrow_mut() = name.to_string();
    }

    /// Drains all queued responses. Called by the play loop after dispatch.
    pub fn drain_responses(&self) -> Vec<Response> {
        self.responses.drain()
    }
}

impl Context for ServerContext {
    fn logger(&self) -> PluginLogger {
        PluginLogger::new(&self.plugin_name.borrow())
    }

    fn player(&self) -> &dyn PlayerContext {
        self
    }

    fn chat(&self) -> &dyn ChatContext {
        self
    }

    fn world_ctx(&self) -> &dyn WorldContext {
        self
    }

    fn entities(&self) -> &dyn EntityContext {
        self
    }

    fn containers(&self) -> &dyn ContainerContext {
        self
    }
}
