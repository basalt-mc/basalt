//! Generic event bus with staged handler dispatch.
//!
//! The [`EventBus`] stores handlers indexed by event `TypeId` and
//! sorted by `(Stage, priority)`. Registration is typed — handlers
//! receive concrete event and context references. Dispatch is a
//! single linear pass through pre-sorted handlers, with short-circuit
//! on cancellation after the Validate stage.

use std::any::TypeId;
use std::collections::HashMap;

use super::traits::{Event, Stage};

/// A type-erased handler function stored in the bus.
///
/// Takes the event as `&mut dyn Event` and a `&dyn Context` reference.
/// The concrete event type is recovered via `downcast_mut` inside the
/// wrapper closure created by [`EventBus::on`].
type ErasedHandler = Box<dyn Fn(&mut dyn Event, &dyn crate::context::Context) + Send + Sync>;

/// A registered handler with its stage and priority.
struct HandlerEntry {
    /// Which stage this handler runs in.
    stage: Stage,
    /// Priority within the stage. Lower values run first.
    priority: i32,
    /// The type-erased handler function.
    handler: ErasedHandler,
}

/// Generic event bus that dispatches events through staged handlers.
///
/// Handlers register for specific event types at specific stages.
/// During dispatch, handlers run in `(Stage, priority)` order.
/// If any Validate handler cancels the event, Process and Post
/// handlers are skipped.
///
/// The bus is `Send + Sync` and can be shared via `Arc` across
/// connection tasks. Registration happens at startup; dispatch
/// happens per-task.
pub struct EventBus {
    /// Handlers indexed by the `TypeId` of the concrete event type.
    /// Each entry list is pre-sorted by `(Stage, priority)`.
    handlers: HashMap<TypeId, Vec<HandlerEntry>>,
}

impl EventBus {
    /// Creates an empty event bus with no registered handlers.
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Registers a handler for a specific event type at a given stage.
    ///
    /// The handler receives a mutable reference to the concrete event
    /// and a `&dyn Context` reference. Lower priority values run first
    /// within the same stage.
    ///
    /// `E` must be `'static` for type erasure via `Any`. The handler
    /// closure must be `Send + Sync` since the bus is shared across
    /// connection tasks.
    pub fn on<E>(
        &mut self,
        stage: Stage,
        priority: i32,
        handler: impl Fn(&mut E, &dyn crate::context::Context) + Send + Sync + 'static,
    ) where
        E: Event + 'static,
    {
        let type_id = TypeId::of::<E>();
        let erased: ErasedHandler = Box::new(move |event, ctx| {
            let concrete_event = event.as_any_mut().downcast_mut::<E>().unwrap();
            handler(concrete_event, ctx);
        });

        let entries = self.handlers.entry(type_id).or_default();
        entries.push(HandlerEntry {
            stage,
            priority,
            handler: erased,
        });
        // Keep entries sorted by (stage, priority) for linear dispatch
        entries.sort_by_key(|e| (e.stage, e.priority));
    }

    /// Dispatches a concrete event through all registered handlers.
    ///
    /// Runs handlers in `(Stage, priority)` order. If the event is
    /// cancelled during Validate, Process and Post are skipped.
    pub fn dispatch<E>(&self, event: &mut E, ctx: &dyn crate::context::Context)
    where
        E: Event + 'static,
    {
        let type_id = TypeId::of::<E>();
        let Some(entries) = self.handlers.get(&type_id) else {
            return;
        };

        for entry in entries {
            if event.is_cancelled() && entry.stage != Stage::Validate {
                return;
            }
            (entry.handler)(event, ctx);
        }
    }

    /// Dispatches a type-erased event using its runtime `TypeId`.
    ///
    /// Used when the concrete event type is not known at the call
    /// site (e.g., `Box<dyn Event>` from `packet_to_event`).
    pub fn dispatch_dyn(&self, event: &mut dyn Event, ctx: &dyn crate::context::Context) {
        let type_id = event.as_any().type_id();
        let Some(entries) = self.handlers.get(&type_id) else {
            return;
        };

        for entry in entries {
            if event.is_cancelled() && entry.stage != Stage::Validate {
                return;
            }
            (entry.handler)(event, ctx);
        }
    }

    /// Returns the number of event types that have registered handlers.
    pub fn event_type_count(&self) -> usize {
        self.handlers.len()
    }

    /// Returns the total number of registered handler entries.
    pub fn handler_count(&self) -> usize {
        self.handlers.values().map(|v| v.len()).sum()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    // EventBus tests are temporarily disabled — they used arbitrary
    // context types like () or stub structs as the C type parameter.
    // After dropping the generic C parameter, the bus requires &dyn
    // Context. These tests are restored in Task 9 once MockContext
    // is defined as part of the testing harness rewrite.
    //
    // See docs/superpowers/plans/2026-04-26-finish-api-standalone.md Task 9.
}
