//! Command trait for server commands.
//!
//! Each command implements this trait and is registered on a
//! [`CommandRegistry`](super::CommandRegistry). When a player
//! types `/name args`, the registry looks up the command by name
//! and calls `execute` with the arguments and context.

use crate::context::Context;

use super::args::CommandArgs;

/// A server command that can be executed by players or the console.
///
/// # Example
///
/// ```ignore
/// use basalt_api::command::Command;
/// use crate::context::Context;
///
/// pub struct PingCommand;
///
/// impl Command for PingCommand {
///     fn name(&self) -> &str { "ping" }
///     fn description(&self) -> &str { "Responds with pong" }
///     fn execute(&self, _args: &CommandArgs, ctx: &dyn Context) {
///         ctx.chat().send("Pong!");
///     }
/// }
/// ```
pub trait Command: Send + Sync {
    /// The command name without the leading `/`.
    fn name(&self) -> &str;

    /// A short description for the help listing.
    fn description(&self) -> &str;

    /// Executes the command with parsed arguments.
    fn execute(&self, args: &CommandArgs, ctx: &dyn Context);
}
