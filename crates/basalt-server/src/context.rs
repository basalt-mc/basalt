//! Event dispatch context and response queue.
//!
//! The [`EventContext`] is created per-dispatch on the stack and passed
//! to all handlers. It provides read access to server state and a
//! [`ResponseQueue`] for handlers to defer async work (sending packets,
//! broadcasting messages).
//!
//! After event dispatch completes, the play loop drains the response
//! queue and executes each response with access to the connection.

use std::cell::RefCell;
use std::sync::Arc;

use basalt_types::nbt::NbtCompound;

use crate::state::{BroadcastMessage, ServerState};

/// Context passed to event handlers during dispatch.
///
/// Created per-dispatch, owned (no borrows). Contains `Arc<ServerState>`
/// for shared state access and a `ResponseQueue` with interior
/// mutability for queueing async work from sync handlers.
///
/// `EventContext` is `'static` (no borrows) so it is compatible with
/// `Any::downcast` in the event bus.
pub struct EventContext {
    /// Shared server state (world, broadcast, player registry).
    pub state: Arc<ServerState>,
    /// Queue for deferred async responses.
    pub responses: ResponseQueue,
}

impl EventContext {
    /// Creates a new context for a single event dispatch.
    pub fn new(state: Arc<ServerState>) -> Self {
        Self {
            state,
            responses: ResponseQueue::new(),
        }
    }
}

/// Thread-local queue for deferred async responses.
///
/// Uses `RefCell` for interior mutability — handlers receive
/// `&EventContext` (shared reference) but need to push responses.
/// This is safe because dispatch is single-threaded within a
/// connection task.
pub struct ResponseQueue {
    /// Interior-mutable response list.
    inner: RefCell<Vec<Response>>,
}

impl ResponseQueue {
    /// Creates an empty response queue.
    pub fn new() -> Self {
        Self {
            inner: RefCell::new(Vec::new()),
        }
    }

    /// Pushes a response onto the queue.
    ///
    /// Called by handlers during dispatch to defer async work.
    pub fn push(&self, response: Response) {
        self.inner.borrow_mut().push(response);
    }

    /// Drains all queued responses, returning them as a Vec.
    ///
    /// Called by the play loop after dispatch completes.
    pub fn drain(&self) -> Vec<Response> {
        self.inner.borrow_mut().drain(..).collect()
    }
}

impl Default for ResponseQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// A deferred async operation queued by a sync event handler.
///
/// After event dispatch completes, the play loop drains the response
/// queue and executes each response with access to the connection.
#[derive(Debug, Clone)]
pub enum Response {
    /// Broadcast a message to all connected players.
    Broadcast(BroadcastMessage),
    /// Send a block action acknowledgement to the current player.
    SendBlockAck {
        /// Sequence number matching the client's dig/place packet.
        sequence: i32,
    },
    /// Send a system chat message to the current player.
    SendSystemChat {
        /// The formatted text component as NBT.
        content: NbtCompound,
        /// Whether to display as an action bar message.
        action_bar: bool,
    },
    /// Teleport the current player to a new position.
    SendPosition {
        /// Teleport ID for confirmation tracking.
        teleport_id: i32,
        /// Target X coordinate.
        x: f64,
        /// Target Y coordinate.
        y: f64,
        /// Target Z coordinate.
        z: f64,
        /// Target yaw angle.
        yaw: f32,
        /// Target pitch angle.
        pitch: f32,
    },
    /// Stream chunks around a new chunk position.
    StreamChunks {
        /// New chunk X coordinate.
        new_cx: i32,
        /// New chunk Z coordinate.
        new_cz: i32,
    },
    /// Send a game state change to the current player.
    SendGameStateChange {
        /// Reason code (e.g., 3 = change gamemode).
        reason: u8,
        /// Value associated with the reason.
        value: f32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_queue_push_and_drain() {
        let queue = ResponseQueue::new();
        queue.push(Response::SendBlockAck { sequence: 1 });
        queue.push(Response::SendBlockAck { sequence: 2 });

        let responses = queue.drain();
        assert_eq!(responses.len(), 2);

        // Second drain is empty
        assert!(queue.drain().is_empty());
    }

    #[test]
    fn response_queue_default_is_empty() {
        let queue = ResponseQueue::default();
        assert!(queue.drain().is_empty());
    }

    #[test]
    fn event_context_creation() {
        let state = ServerState::new();
        let ctx = EventContext::new(Arc::clone(&state));
        assert!(ctx.responses.drain().is_empty());
    }
}
