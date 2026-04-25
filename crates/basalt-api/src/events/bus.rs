//! Generic event bus with staged handler dispatch.
//!
//! The [`EventBus`] stores handlers indexed by event `TypeId` and
//! sorted by `(Stage, priority)`. Registration is typed — handlers
//! receive concrete event and context references. Dispatch is a
//! single linear pass through pre-sorted handlers, with short-circuit
//! on cancellation after the Validate stage.

use std::any::{Any, TypeId};
use std::collections::HashMap;

use super::traits::{Event, Stage};

/// A type-erased handler function stored in the bus.
///
/// Takes the event as `&mut dyn Event` and context as `&dyn Any`.
/// The concrete types are recovered via `downcast_mut`/`downcast_ref`
/// inside the wrapper closure created by `on()`.
type ErasedHandler = Box<dyn Fn(&mut dyn Event, &dyn Any) + Send + Sync>;

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
    /// and a shared reference to the context. Lower priority values
    /// run first within the same stage.
    ///
    /// Both `E` (event) and `C` (context) must be `'static` for
    /// type erasure via `Any`. The handler closure must be `Send +
    /// Sync` since the bus is shared across connection tasks.
    pub fn on<E, C>(
        &mut self,
        stage: Stage,
        priority: i32,
        handler: impl Fn(&mut E, &C) + Send + Sync + 'static,
    ) where
        E: Event + 'static,
        C: 'static,
    {
        let type_id = TypeId::of::<E>();
        let erased: ErasedHandler = Box::new(move |event, ctx_any| {
            let concrete_event = event.as_any_mut().downcast_mut::<E>().unwrap();
            let concrete_ctx = ctx_any.downcast_ref::<C>().unwrap();
            handler(concrete_event, concrete_ctx);
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
    pub fn dispatch<E, C>(&self, event: &mut E, ctx: &C)
    where
        E: Event + 'static,
        C: 'static,
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
    pub fn dispatch_dyn<C: 'static>(&self, event: &mut dyn Event, ctx: &C) {
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
            BusKind::Instant
        }
    }

    struct OtherEvent {
        tag: String,
    }

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

    // -- Tests --

    #[test]
    fn empty_bus_dispatch_does_nothing() {
        let bus = EventBus::new();
        let mut event = CounterEvent {
            value: 0,
            cancelled: false,
        };
        bus.dispatch(&mut event, &());
        assert_eq!(event.value, 0);
    }

    #[test]
    fn single_handler_modifies_event() {
        let mut bus = EventBus::new();
        bus.on::<CounterEvent, ()>(Stage::Process, 0, |event, _| {
            event.value += 10;
        });

        let mut event = CounterEvent {
            value: 0,
            cancelled: false,
        };
        bus.dispatch(&mut event, &());
        assert_eq!(event.value, 10);
    }

    #[test]
    fn handlers_run_in_stage_order() {
        let mut bus = EventBus::new();

        // Register in reverse order to verify sorting
        bus.on::<CounterEvent, ()>(Stage::Post, 0, |event, _| {
            event.value += 1000;
        });
        bus.on::<CounterEvent, ()>(Stage::Validate, 0, |event, _| {
            event.value += 1;
        });
        bus.on::<CounterEvent, ()>(Stage::Process, 0, |event, _| {
            // Verify Validate already ran
            assert_eq!(event.value, 1);
            event.value += 100;
        });

        let mut event = CounterEvent {
            value: 0,
            cancelled: false,
        };
        bus.dispatch(&mut event, &());
        assert_eq!(event.value, 1101);
    }

    #[test]
    fn priority_within_stage() {
        let mut bus = EventBus::new();

        // Higher priority (lower number) runs first
        bus.on::<CounterEvent, ()>(Stage::Process, 10, |event, _| {
            event.value *= 2;
        });
        bus.on::<CounterEvent, ()>(Stage::Process, 0, |event, _| {
            event.value += 5;
        });

        let mut event = CounterEvent {
            value: 0,
            cancelled: false,
        };
        bus.dispatch(&mut event, &());
        // Priority 0 runs first: 0 + 5 = 5
        // Priority 10 runs second: 5 * 2 = 10
        assert_eq!(event.value, 10);
    }

    #[test]
    fn validate_cancellation_skips_process_and_post() {
        let mut bus = EventBus::new();

        bus.on::<CounterEvent, ()>(Stage::Validate, 0, |event, _| {
            event.cancel();
        });
        bus.on::<CounterEvent, ()>(Stage::Process, 0, |event, _| {
            event.value = 999; // should NOT run
        });
        bus.on::<CounterEvent, ()>(Stage::Post, 0, |event, _| {
            event.value = 888; // should NOT run
        });

        let mut event = CounterEvent {
            value: 0,
            cancelled: false,
        };
        bus.dispatch(&mut event, &());

        assert!(event.is_cancelled());
        assert_eq!(event.value, 0);
    }

    #[test]
    fn cancelled_event_still_runs_remaining_validate_handlers() {
        let mut bus = EventBus::new();

        bus.on::<CounterEvent, ()>(Stage::Validate, 0, |event, _| {
            event.cancel();
        });
        bus.on::<CounterEvent, ()>(Stage::Validate, 10, |event, _| {
            // This Validate handler still runs even though cancelled
            event.value += 1;
        });

        let mut event = CounterEvent {
            value: 0,
            cancelled: false,
        };
        bus.dispatch(&mut event, &());
        assert!(event.is_cancelled());
        assert_eq!(event.value, 1);
    }

    #[test]
    fn different_event_types_are_independent() {
        let mut bus = EventBus::new();

        bus.on::<CounterEvent, ()>(Stage::Process, 0, |event, _| {
            event.value += 1;
        });
        bus.on::<OtherEvent, ()>(Stage::Process, 0, |event, _| {
            event.tag = "modified".into();
        });

        let mut counter = CounterEvent {
            value: 0,
            cancelled: false,
        };
        let mut other = OtherEvent {
            tag: "original".into(),
        };

        bus.dispatch(&mut counter, &());
        bus.dispatch(&mut other, &());

        assert_eq!(counter.value, 1);
        assert_eq!(other.tag, "modified");
    }

    #[test]
    fn dispatch_dyn_works() {
        let mut bus = EventBus::new();
        bus.on::<CounterEvent, ()>(Stage::Process, 0, |event, _| {
            event.value += 42;
        });

        let mut event = CounterEvent {
            value: 0,
            cancelled: false,
        };
        // Use dispatch_dyn with &mut dyn Event
        bus.dispatch_dyn(&mut event as &mut dyn Event, &());
        assert_eq!(event.value, 42);
    }

    #[test]
    fn dispatch_dyn_respects_cancellation() {
        let mut bus = EventBus::new();
        bus.on::<CounterEvent, ()>(Stage::Validate, 0, |event, _| {
            event.cancel();
        });
        bus.on::<CounterEvent, ()>(Stage::Process, 0, |event, _| {
            event.value = 999;
        });

        let mut event = CounterEvent {
            value: 0,
            cancelled: false,
        };
        bus.dispatch_dyn(&mut event as &mut dyn Event, &());
        assert!(event.is_cancelled());
        assert_eq!(event.value, 0);
    }

    #[test]
    fn context_is_passed_to_handlers() {
        struct Ctx {
            multiplier: i32,
        }

        let mut bus = EventBus::new();
        bus.on::<CounterEvent, Ctx>(Stage::Process, 0, |event, ctx| {
            event.value *= ctx.multiplier;
        });

        let mut event = CounterEvent {
            value: 5,
            cancelled: false,
        };
        let ctx = Ctx { multiplier: 3 };
        bus.dispatch(&mut event, &ctx);
        assert_eq!(event.value, 15);
    }

    #[test]
    fn event_type_count_and_handler_count() {
        let mut bus = EventBus::new();
        assert_eq!(bus.event_type_count(), 0);
        assert_eq!(bus.handler_count(), 0);

        bus.on::<CounterEvent, ()>(Stage::Validate, 0, |_, _| {});
        bus.on::<CounterEvent, ()>(Stage::Process, 0, |_, _| {});
        bus.on::<OtherEvent, ()>(Stage::Post, 0, |_, _| {});

        assert_eq!(bus.event_type_count(), 2);
        assert_eq!(bus.handler_count(), 3);
    }

    #[test]
    fn multiple_handlers_same_stage_same_priority() {
        let mut bus = EventBus::new();

        bus.on::<CounterEvent, ()>(Stage::Process, 0, |event, _| {
            event.value += 1;
        });
        bus.on::<CounterEvent, ()>(Stage::Process, 0, |event, _| {
            event.value += 1;
        });

        let mut event = CounterEvent {
            value: 0,
            cancelled: false,
        };
        bus.dispatch(&mut event, &());
        assert_eq!(event.value, 2);
    }

    #[test]
    fn non_cancellable_event_ignores_cancel() {
        let mut bus = EventBus::new();
        bus.on::<OtherEvent, ()>(Stage::Validate, 0, |event, _| {
            event.cancel(); // no-op for OtherEvent
        });
        bus.on::<OtherEvent, ()>(Stage::Process, 0, |event, _| {
            event.tag = "processed".into();
        });

        let mut event = OtherEvent {
            tag: "original".into(),
        };
        bus.dispatch(&mut event, &());
        // Process should have run because cancel() was a no-op
        assert_eq!(event.tag, "processed");
    }

    #[test]
    fn default_creates_empty_bus() {
        let bus = EventBus::default();
        assert_eq!(bus.event_type_count(), 0);
    }
}
