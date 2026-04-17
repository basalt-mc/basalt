//! Game loop — single dedicated OS thread for tick-based simulation.

mod tick;

pub(crate) use tick::GameLoop;
