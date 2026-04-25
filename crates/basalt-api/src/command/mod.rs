//! Command system: argument types, parsing, validation, dispatch.
//!
//! Plugins declare command arguments with types. The framework
//! handles parsing, validation, error messages, DeclareCommands
//! generation, and TabComplete responses. Built-in plugins use the
//! fluent builder API on [`PluginRegistrar`](crate::PluginRegistrar);
//! the [`Command`] trait + [`CommandRegistry`] are an alternative
//! API for plugins that prefer trait-based dispatch.

pub mod args;
mod dispatch;
mod registry;

pub use args::{
    Arg, ArgValue, CommandArg, CommandArgs, Validation, parse_args, parse_command_args,
};
pub use dispatch::Command;
pub use registry::CommandRegistry;
