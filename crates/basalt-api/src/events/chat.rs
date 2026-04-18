//! Chat and command events.

/// A player sent a chat message.
///
/// If cancelled, the message is not broadcast. The sender is
/// available via `ctx.player()`.
#[derive(Debug, Clone)]
pub struct ChatMessageEvent {
    /// The chat message content.
    pub message: String,
    /// Whether this event has been cancelled by a Validate handler.
    pub cancelled: bool,
}
crate::instant_cancellable_event!(ChatMessageEvent);

/// A player issued a command (e.g., `/tp 0 64 0`).
///
/// If cancelled, the command is not executed. The issuing player
/// is available via `ctx.player()`.
#[derive(Debug, Clone)]
pub struct CommandEvent {
    /// The command string without the leading `/`.
    pub command: String,
    /// Whether this event has been cancelled by a Validate handler.
    pub cancelled: bool,
}
crate::instant_cancellable_event!(CommandEvent);

#[cfg(test)]
mod tests {
    use basalt_events::{BusKind, EventRouting};

    use super::*;

    #[test]
    fn event_routing() {
        assert_eq!(ChatMessageEvent::BUS, BusKind::Instant);
        assert_eq!(CommandEvent::BUS, BusKind::Instant);
    }
}
