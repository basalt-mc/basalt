//! Event trait and execution stages.
//!
//! Events are typed structs carrying game data (positions, UUIDs,
//! block states) and a cancellation flag. The `Event` trait provides
//! type erasure via `Any` so the [`EventBus`](crate::EventBus) can
//! store handlers for different event types in a single registry.

use std::any::Any;

/// Execution stage for event handlers.
///
/// Handlers run in stage order: Validate → Process → Post.
/// If any Validate handler cancels the event, Process and Post
/// are skipped entirely.
///
/// - **Validate**: read-only checks, can cancel (permissions, anti-cheat)
/// - **Process**: state mutation, one logical owner (world changes)
/// - **Post**: side effects, never cancels (broadcast, storage, logging)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    /// Validation stage: read-only, can cancel. Runs first.
    Validate,
    /// Processing stage: mutates state. Runs second.
    Process,
    /// Post-processing stage: side effects. Runs last.
    Post,
}

/// Trait implemented by all game events.
///
/// Events carry domain data and support cancellation. The `as_any`
/// methods enable type erasure inside the `EventBus` — handlers
/// register for concrete types via `TypeId`, and the bus downcasts
/// during dispatch.
///
/// Not all events are cancellable. For non-cancellable events
/// (e.g., `PlayerJoinedEvent`), `cancel()` is a no-op and
/// `is_cancelled()` always returns `false`.
pub trait Event: Any + Send {
    /// Whether this event has been cancelled by a Validate handler.
    fn is_cancelled(&self) -> bool;

    /// Cancels this event. Process and Post handlers will be skipped.
    ///
    /// Only meaningful during the Validate stage. Non-cancellable
    /// events ignore this call.
    fn cancel(&mut self);

    /// Upcasts to `&dyn Any` for type-erased dispatch.
    fn as_any(&self) -> &dyn Any;

    /// Upcasts to `&mut dyn Any` for mutable type-erased dispatch.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestEvent {
        value: i32,
        cancelled: bool,
    }

    impl Event for TestEvent {
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
    }

    #[test]
    fn event_cancellation() {
        let mut event = TestEvent {
            value: 42,
            cancelled: false,
        };
        assert!(!event.is_cancelled());
        event.cancel();
        assert!(event.is_cancelled());
        assert_eq!(event.value, 42);
    }

    #[test]
    fn event_downcast() {
        let mut event = TestEvent {
            value: 99,
            cancelled: false,
        };
        let any = event.as_any_mut();
        let concrete = any.downcast_mut::<TestEvent>().unwrap();
        concrete.value = 100;
        assert_eq!(event.value, 100);
    }

    #[test]
    fn stage_ordering() {
        assert!(Stage::Validate < Stage::Process);
        assert!(Stage::Process < Stage::Post);
    }
}
