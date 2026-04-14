//! Command trait for server commands.
//!
//! Each command implements this trait and is registered on a
//! [`CommandRegistry`](crate::CommandRegistry). When a player
//! types `/name args`, the registry looks up the command by name
//! and calls `execute` with the arguments and server context.

use basalt_api::context::ServerContext;

/// A server command that can be executed by players.
///
/// # Example
///
/// ```ignore
/// use basalt_command::Command;
/// use basalt_api::context::ServerContext;
///
/// pub struct PingCommand;
///
/// impl Command for PingCommand {
///     fn name(&self) -> &str { "ping" }
///     fn description(&self) -> &str { "Responds with pong" }
///     fn execute(&self, _args: &str, ctx: &ServerContext) {
///         ctx.send_message("Pong!");
///     }
/// }
/// ```
pub trait Command: Send + Sync {
    /// The command name without the leading `/`.
    fn name(&self) -> &str;

    /// A short description for the help listing.
    fn description(&self) -> &str;

    /// Executes the command with the given arguments.
    ///
    /// `args` is the remainder of the command string after the name,
    /// e.g., for `/tp 10 64 -5`, args is `"10 64 -5"`.
    /// Empty string if the command was invoked without arguments.
    fn execute(&self, args: &str, ctx: &ServerContext);
}
