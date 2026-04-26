use basalt_mc_protocol::ConnectionState;

/// The result of a middleware processing a packet.
///
/// Determines how the pipeline proceeds after a middleware runs.
/// The chain stops immediately on `Drop` or `Reply` — subsequent
/// middlewares are not called.
#[derive(Debug, PartialEq)]
pub enum Action {
    /// Pass the packet unchanged to the next middleware in the chain.
    /// If this is the last middleware, the packet reaches the handler.
    Continue,

    /// The packet was modified by this middleware. Continue processing
    /// with the modified packet. Subsequent middlewares see the changes.
    ModifiedContinue,

    /// Silently drop the packet. No further middlewares run, and the
    /// handler never sees it. Useful for filtering, anti-cheat, or
    /// rate limiting.
    Drop,

    /// Reply immediately with raw packet bytes and stop the chain.
    /// The reply is sent back to the sender, and the original packet
    /// is consumed. Useful for auto-responses (e.g., ping handlers).
    Reply(Vec<u8>),
}

/// Context passed to middlewares during packet processing.
///
/// Provides mutable access to the packet data so middlewares can inspect
/// or modify it. Also carries metadata about the connection state and
/// packet direction for context-aware processing.
pub struct PacketContext {
    /// The VarInt packet ID.
    pub packet_id: i32,

    /// The raw packet payload bytes (after the packet ID).
    /// Middlewares can read or modify this data.
    pub payload: Vec<u8>,

    /// The current connection state when this packet was received/sent.
    pub state: ConnectionState,

    /// Whether this packet is incoming (from client) or outgoing (to client).
    pub incoming: bool,
}

/// A packet-level hook that can intercept, modify, drop, or reply to packets.
///
/// Middlewares are executed in priority order by the [`Pipeline`]. Each
/// middleware receives a mutable [`PacketContext`] and returns an [`Action`]
/// that controls whether processing continues.
///
/// Implementations must be `Send + Sync` to support concurrent connections.
/// Processing is synchronous — async work should be offloaded to a task.
pub trait Middleware: Send + Sync {
    /// Called when a packet is received from the remote side.
    ///
    /// Return `Action::Continue` to pass the packet through unchanged,
    /// `Action::ModifiedContinue` if you modified the context, `Action::Drop`
    /// to silently discard the packet, or `Action::Reply` to send a response
    /// and stop processing.
    fn on_incoming(&self, ctx: &mut PacketContext) -> Action;

    /// Called when a packet is about to be sent to the remote side.
    ///
    /// Same action semantics as `on_incoming`. Outgoing middlewares run
    /// in the same priority order as incoming ones.
    fn on_outgoing(&self, ctx: &mut PacketContext) -> Action;
}

/// An entry in the pipeline: a middleware with its priority level.
struct PipelineEntry {
    /// Lower priority values run first.
    priority: i32,
    /// The middleware implementation.
    middleware: Box<dyn Middleware>,
}

/// An ordered chain of middlewares that process packets sequentially.
///
/// Middlewares are sorted by priority (lowest first) and executed in
/// order. The chain stops early if any middleware returns `Drop` or
/// `Reply`. The pipeline is built during server setup and shared
/// across connections.
pub struct Pipeline {
    entries: Vec<PipelineEntry>,
}

impl Pipeline {
    /// Creates a new empty pipeline with no middlewares.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Adds a middleware to the pipeline at the given priority level.
    ///
    /// Lower priority values run first. Middlewares with the same priority
    /// run in insertion order. The pipeline re-sorts after each addition.
    pub fn add(&mut self, middleware: impl Middleware + 'static, priority: i32) {
        self.entries.push(PipelineEntry {
            priority,
            middleware: Box::new(middleware),
        });
        self.entries.sort_by_key(|e| e.priority);
    }

    /// Processes an incoming packet through all middlewares in order.
    ///
    /// Returns the final action after the entire chain has run. If any
    /// middleware returns `Drop` or `Reply`, processing stops and that
    /// action is returned immediately.
    pub fn process_incoming(&self, ctx: &mut PacketContext) -> Action {
        for entry in &self.entries {
            match entry.middleware.on_incoming(ctx) {
                Action::Continue => continue,
                Action::ModifiedContinue => continue,
                action => return action,
            }
        }
        Action::Continue
    }

    /// Processes an outgoing packet through all middlewares in order.
    ///
    /// Same semantics as `process_incoming` but calls `on_outgoing`
    /// on each middleware.
    pub fn process_outgoing(&self, ctx: &mut PacketContext) -> Action {
        for entry in &self.entries {
            match entry.middleware.on_outgoing(ctx) {
                Action::Continue => continue,
                Action::ModifiedContinue => continue,
                action => return action,
            }
        }
        Action::Continue
    }

    /// Returns the number of middlewares in the pipeline.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the pipeline has no middlewares.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A middleware that passes everything through unchanged.
    struct PassthroughMiddleware;
    impl Middleware for PassthroughMiddleware {
        fn on_incoming(&self, _ctx: &mut PacketContext) -> Action {
            Action::Continue
        }
        fn on_outgoing(&self, _ctx: &mut PacketContext) -> Action {
            Action::Continue
        }
    }

    /// A middleware that drops all incoming packets.
    struct DropMiddleware;
    impl Middleware for DropMiddleware {
        fn on_incoming(&self, _ctx: &mut PacketContext) -> Action {
            Action::Drop
        }
        fn on_outgoing(&self, _ctx: &mut PacketContext) -> Action {
            Action::Continue
        }
    }

    /// A middleware that appends a byte to the payload (modification).
    struct AppendMiddleware {
        byte: u8,
    }
    impl Middleware for AppendMiddleware {
        fn on_incoming(&self, ctx: &mut PacketContext) -> Action {
            ctx.payload.push(self.byte);
            Action::ModifiedContinue
        }
        fn on_outgoing(&self, ctx: &mut PacketContext) -> Action {
            ctx.payload.push(self.byte);
            Action::ModifiedContinue
        }
    }

    /// A middleware that replies with fixed data.
    struct ReplyMiddleware {
        reply: Vec<u8>,
    }
    impl Middleware for ReplyMiddleware {
        fn on_incoming(&self, _ctx: &mut PacketContext) -> Action {
            Action::Reply(self.reply.clone())
        }
        fn on_outgoing(&self, _ctx: &mut PacketContext) -> Action {
            Action::Continue
        }
    }

    /// A middleware that records whether it was called (via payload mutation).
    struct RecordMiddleware {
        marker: u8,
    }
    impl Middleware for RecordMiddleware {
        fn on_incoming(&self, ctx: &mut PacketContext) -> Action {
            ctx.payload.push(self.marker);
            Action::ModifiedContinue
        }
        fn on_outgoing(&self, _ctx: &mut PacketContext) -> Action {
            Action::Continue
        }
    }

    fn make_ctx() -> PacketContext {
        PacketContext {
            packet_id: 0x00,
            payload: vec![],
            state: ConnectionState::Play,
            incoming: true,
        }
    }

    // -- Pipeline tests --

    #[test]
    fn empty_pipeline() {
        let pipeline = Pipeline::new();
        assert!(pipeline.is_empty());
        assert_eq!(pipeline.len(), 0);

        let mut ctx = make_ctx();
        assert_eq!(pipeline.process_incoming(&mut ctx), Action::Continue);
    }

    #[test]
    fn passthrough() {
        let mut pipeline = Pipeline::new();
        pipeline.add(PassthroughMiddleware, 0);

        let mut ctx = make_ctx();
        assert_eq!(pipeline.process_incoming(&mut ctx), Action::Continue);
        assert!(ctx.payload.is_empty());
    }

    #[test]
    fn drop_stops_chain() {
        let mut pipeline = Pipeline::new();
        pipeline.add(DropMiddleware, 0);
        pipeline.add(AppendMiddleware { byte: 0xFF }, 1);

        let mut ctx = make_ctx();
        assert_eq!(pipeline.process_incoming(&mut ctx), Action::Drop);
        // Second middleware should NOT have run
        assert!(ctx.payload.is_empty());
    }

    #[test]
    fn reply_stops_chain() {
        let mut pipeline = Pipeline::new();
        pipeline.add(
            ReplyMiddleware {
                reply: vec![0x01, 0x02],
            },
            0,
        );
        pipeline.add(AppendMiddleware { byte: 0xFF }, 1);

        let mut ctx = make_ctx();
        let action = pipeline.process_incoming(&mut ctx);
        assert_eq!(action, Action::Reply(vec![0x01, 0x02]));
        // Second middleware should NOT have run
        assert!(ctx.payload.is_empty());
    }

    #[test]
    fn modification_propagates() {
        let mut pipeline = Pipeline::new();
        pipeline.add(AppendMiddleware { byte: 0xAA }, 0);
        pipeline.add(AppendMiddleware { byte: 0xBB }, 1);

        let mut ctx = make_ctx();
        pipeline.process_incoming(&mut ctx);
        // Both middlewares should have appended their bytes
        assert_eq!(ctx.payload, vec![0xAA, 0xBB]);
    }

    #[test]
    fn priority_ordering() {
        let mut pipeline = Pipeline::new();
        // Add in reverse order — should still execute by priority
        pipeline.add(RecordMiddleware { marker: 0x03 }, 30);
        pipeline.add(RecordMiddleware { marker: 0x01 }, 10);
        pipeline.add(RecordMiddleware { marker: 0x02 }, 20);

        let mut ctx = make_ctx();
        pipeline.process_incoming(&mut ctx);
        // Should be sorted by priority: 10, 20, 30
        assert_eq!(ctx.payload, vec![0x01, 0x02, 0x03]);
    }

    #[test]
    fn outgoing_processing() {
        let mut pipeline = Pipeline::new();
        pipeline.add(AppendMiddleware { byte: 0xCC }, 0);

        let mut ctx = make_ctx();
        ctx.incoming = false;
        pipeline.process_outgoing(&mut ctx);
        assert_eq!(ctx.payload, vec![0xCC]);
    }

    #[test]
    fn drop_only_affects_incoming() {
        let mut pipeline = Pipeline::new();
        pipeline.add(DropMiddleware, 0);

        // Incoming is dropped
        let mut ctx = make_ctx();
        assert_eq!(pipeline.process_incoming(&mut ctx), Action::Drop);

        // Outgoing passes through
        let mut ctx = make_ctx();
        ctx.incoming = false;
        assert_eq!(pipeline.process_outgoing(&mut ctx), Action::Continue);
    }
}
