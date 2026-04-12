//! Basalt event system with staged handler dispatch.
//!
//! Provides a generic [`EventBus`] that dispatches typed events
//! through prioritized handlers organized in three execution stages:
//!
//! 1. **Validate** — read-only checks, can cancel (permissions, anti-cheat)
//! 2. **Process** — state mutation (world changes, inventory updates)
//! 3. **Post** — side effects (broadcasting, persistence, logging)
//!
//! If any Validate handler cancels an event, Process and Post are
//! skipped entirely. This enables permission and protection plugins
//! without modifying game logic.
//!
//! # Design
//!
//! - **Sync handlers**: all handlers are synchronous. Async work is
//!   deferred through a response queue in the caller.
//! - **Type erasure**: handlers register for concrete event types via
//!   `TypeId`. The bus downcasts during dispatch.
//! - **Zero dependencies**: this crate has no external dependencies.
//! - **Generic context**: handlers receive a context reference that
//!   the caller defines (e.g., `EventContext` in `basalt-server`).

mod bus;
mod event;

pub use bus::EventBus;
pub use event::{Event, Stage};
