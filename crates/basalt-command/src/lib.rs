//! Basalt command system.
//!
//! Provides the [`Command`] trait and [`CommandRegistry`] for
//! registering and dispatching server commands. Commands are
//! typically registered at startup by a `CommandPlugin` and
//! executed when players type `/name args` in chat.
//!
//! # Writing a command
//!
//! ```ignore
//! use basalt_command::Command;
//! use basalt_api::context::ServerContext;
//!
//! pub struct HomeCommand;
//!
//! impl Command for HomeCommand {
//!     fn name(&self) -> &str { "home" }
//!     fn description(&self) -> &str { "Teleport to spawn" }
//!     fn execute(&self, _args: &str, ctx: &ServerContext) {
//!         ctx.teleport(0.0, 64.0, 0.0, 0.0, 0.0);
//!         ctx.send_message("Teleported home!");
//!     }
//! }
//! ```

mod command;
mod registry;

pub use command::Command;
pub use registry::CommandRegistry;
