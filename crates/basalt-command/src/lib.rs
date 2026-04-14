//! Basalt command system.
//!
//! Provides the [`Command`] trait, argument types with parsing and
//! validation, and the [`CommandRegistry`] for dispatch.
//!
//! This crate depends on `basalt-core` for the [`Context`](basalt_core::Context)
//! trait — it does NOT depend on `basalt-api`.

pub mod args;
mod command;
mod registry;

pub use args::{
    Arg, ArgValue, CommandArg, CommandArgs, Validation, parse_args, parse_command_args,
};
pub use command::Command;
pub use registry::CommandRegistry;
