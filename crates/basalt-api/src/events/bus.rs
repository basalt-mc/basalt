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
    use super::*;
    use crate::events::traits::BusKind;
    use crate::testing::NoopContext;
    use std::any::Any;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI32, Ordering};

    // -- Test event types --

    struct CounterEvent {
        value: i32,
        cancelled: bool,
    }

    impl Event for CounterEvent {
        fn is_cancelled(&self) -> bool {
            self.cancelled
        }
        fn cancel(&mut self) {
            self.cancelled = true;
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
        fn bus_kind(&self) -> BusKind {
            BusKind::Game
        }
    }

    struct OtherEvent;

    impl Event for OtherEvent {
        fn is_cancelled(&self) -> bool {
            false
        }
        fn cancel(&mut self) {}
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
        fn bus_kind(&self) -> BusKind {
            BusKind::Game
        }
    }

    #[test]
    fn dispatch_runs_handlers_in_priority_order() {
        let mut bus = EventBus::new();
        let order = Arc::new(std::sync::Mutex::new(Vec::<i32>::new()));

        for &priority in &[10, 5, 1, 20] {
            let order_ref = Arc::clone(&order);
            bus.on::<CounterEvent>(Stage::Process, priority, move |_, _| {
                order_ref.lock().unwrap().push(priority);
            });
        }

        let mut event = CounterEvent {
            value: 0,
            cancelled: false,
        };
        bus.dispatch(&mut event, &NoopContext);

        assert_eq!(*order.lock().unwrap(), vec![1, 5, 10, 20]);
    }

    #[test]
    fn dispatch_skips_post_when_cancelled_in_validate() {
        let mut bus = EventBus::new();
        let post_ran = Arc::new(AtomicI32::new(0));

        bus.on::<CounterEvent>(Stage::Validate, 0, |event, _| {
            event.cancel();
        });
        let post_ref = Arc::clone(&post_ran);
        bus.on::<CounterEvent>(Stage::Post, 0, move |_, _| {
            post_ref.fetch_add(1, Ordering::Relaxed);
        });

        let mut event = CounterEvent {
            value: 0,
            cancelled: false,
        };
        bus.dispatch(&mut event, &NoopContext);

        assert_eq!(post_ran.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn dispatch_skips_process_when_cancelled_in_validate() {
        let mut bus = EventBus::new();
        let process_ran = Arc::new(AtomicI32::new(0));

        bus.on::<CounterEvent>(Stage::Validate, 0, |event, _| {
            event.cancel();
        });
        let proc_ref = Arc::clone(&process_ran);
        bus.on::<CounterEvent>(Stage::Process, 0, move |_, _| {
            proc_ref.fetch_add(1, Ordering::Relaxed);
        });

        let mut event = CounterEvent {
            value: 0,
            cancelled: false,
        };
        bus.dispatch(&mut event, &NoopContext);

        assert_eq!(process_ran.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn dispatch_runs_remaining_validate_handlers_after_cancel() {
        let mut bus = EventBus::new();
        let count = Arc::new(AtomicI32::new(0));

        // Priority 0: cancel early
        bus.on::<CounterEvent>(Stage::Validate, 0, |event, _| {
            event.cancel();
        });
        // Priority 1: should still run (same stage, higher priority value)
        let c = Arc::clone(&count);
        bus.on::<CounterEvent>(Stage::Validate, 1, move |_, _| {
            c.fetch_add(1, Ordering::Relaxed);
        });

        let mut event = CounterEvent {
            value: 0,
            cancelled: false,
        };
        bus.dispatch(&mut event, &NoopContext);

        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn dispatch_with_no_handlers_is_noop() {
        let bus = EventBus::new();
        let mut event = CounterEvent {
            value: 42,
            cancelled: false,
        };
        bus.dispatch(&mut event, &NoopContext);
        assert_eq!(event.value, 42);
    }

    #[test]
    fn handlers_for_different_events_are_isolated() {
        let mut bus = EventBus::new();
        let counter_ran = Arc::new(AtomicI32::new(0));
        let other_ran = Arc::new(AtomicI32::new(0));

        let cr = Arc::clone(&counter_ran);
        bus.on::<CounterEvent>(Stage::Process, 0, move |_, _| {
            cr.fetch_add(1, Ordering::Relaxed);
        });
        let or = Arc::clone(&other_ran);
        bus.on::<OtherEvent>(Stage::Process, 0, move |_, _| {
            or.fetch_add(1, Ordering::Relaxed);
        });

        let mut counter = CounterEvent {
            value: 0,
            cancelled: false,
        };
        bus.dispatch(&mut counter, &NoopContext);

        assert_eq!(counter_ran.load(Ordering::Relaxed), 1);
        assert_eq!(other_ran.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn dispatch_dyn_routes_by_runtime_type_id() {
        let mut bus = EventBus::new();
        let count = Arc::new(AtomicI32::new(0));

        let c = Arc::clone(&count);
        bus.on::<CounterEvent>(Stage::Process, 0, move |_, _| {
            c.fetch_add(1, Ordering::Relaxed);
        });

        let mut event: Box<dyn Event> = Box::new(CounterEvent {
            value: 0,
            cancelled: false,
        });
        bus.dispatch_dyn(&mut *event, &NoopContext);
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn event_type_count_tracks_distinct_types() {
        let mut bus = EventBus::new();
        assert_eq!(bus.event_type_count(), 0);

        bus.on::<CounterEvent>(Stage::Process, 0, |_, _| {});
        assert_eq!(bus.event_type_count(), 1);

        bus.on::<CounterEvent>(Stage::Post, 0, |_, _| {});
        assert_eq!(bus.event_type_count(), 1);

        bus.on::<OtherEvent>(Stage::Process, 0, |_, _| {});
        assert_eq!(bus.event_type_count(), 2);
    }

    #[test]
    fn handler_count_sums_across_types_and_stages() {
        let mut bus = EventBus::new();
        bus.on::<CounterEvent>(Stage::Process, 0, |_, _| {});
        bus.on::<CounterEvent>(Stage::Post, 0, |_, _| {});
        bus.on::<OtherEvent>(Stage::Process, 0, |_, _| {});
        assert_eq!(bus.handler_count(), 3);
    }
}
