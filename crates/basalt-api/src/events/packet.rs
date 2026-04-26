//! Raw serverbound packet events — pre-dispatch hook for plugins
//! that operate at the wire layer (anti-cheat, telemetry, packet
//! logging, custom protocol gateways).

use basalt_mc_protocol::packets::play::ServerboundPlayPacket;

/// Fires before a serverbound Play packet is dispatched to its
/// domain handler. Cancelling drops the packet — no further
/// processing in the net task or the game loop.
///
/// Runs on the **instant** event bus, on the per-player net task
/// thread, before chat / command dispatch and before forwarding to
/// the game loop. Cancellation is the only side-effect plugins
/// should perform here; mutating the carried packet has no effect
/// on subsequent dispatch (the original is consumed by the handler
/// path independently).
///
/// Plugins typically match `event.packet` to detect a specific
/// packet type and inspect its fields:
///
/// ```text
/// match &event.packet {
///     ServerboundPlayPacket::BlockDig(d) => {
///         if too_far(d.location) { event.cancelled = true; }
///     }
///     _ => {}
/// }
/// ```
///
/// The packet is owned (cloned at dispatch time) so plugins can
/// stash it for later inspection without lifetime gymnastics.
#[derive(Debug, Clone)]
pub struct RawPacketEvent {
    /// The decoded serverbound packet about to be dispatched.
    pub packet: ServerboundPlayPacket,
    /// Set to `true` to drop the packet.
    pub cancelled: bool,
}
crate::instant_cancellable_event!(RawPacketEvent);

#[cfg(test)]
mod tests {
    use crate::events::Event;
    use basalt_mc_protocol::packets::play::ServerboundPlayPacket;
    use basalt_mc_protocol::packets::play::misc::ServerboundPlayKeepAlive;

    use super::*;

    #[test]
    fn raw_packet_event_cancellation() {
        let mut event = RawPacketEvent {
            packet: ServerboundPlayPacket::KeepAlive(ServerboundPlayKeepAlive {
                keep_alive_id: 42,
            }),
            cancelled: false,
        };
        assert!(!event.is_cancelled());
        event.cancel();
        assert!(event.is_cancelled());
    }

    #[test]
    fn raw_packet_event_routes_to_instant_bus() {
        use crate::events::{BusKind, EventRouting};
        assert_eq!(RawPacketEvent::BUS, BusKind::Instant);
    }

    #[test]
    fn raw_packet_event_downcast_roundtrip() {
        let mut event = RawPacketEvent {
            packet: ServerboundPlayPacket::KeepAlive(ServerboundPlayKeepAlive { keep_alive_id: 7 }),
            cancelled: false,
        };
        let any = event.as_any_mut();
        let concrete = any.downcast_mut::<RawPacketEvent>().unwrap();
        assert!(matches!(
            &concrete.packet,
            ServerboundPlayPacket::KeepAlive(ka) if ka.keep_alive_id == 7
        ));
    }
}
