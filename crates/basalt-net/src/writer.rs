//! Packet writer trait for abstracting packet output.
//!
//! `PacketWriter` decouples packet sending from the concrete TCP
//! connection. This enables unit testing of packet-sending code
//! without a real TCP stream — tests can use a mock writer that
//! records packets instead of sending them over the network.

use crate::error::Result;
use basalt_types::{Encode, EncodedSize};

/// Abstraction over writing protocol packets.
///
/// The main implementation is `Connection<Play>`, but tests can
/// provide mock implementations that capture packets for assertions.
#[allow(async_fn_in_trait)]
pub trait PacketWriter {
    /// Writes a typed packet with the given ID to the output.
    async fn write_packet_typed<P: Encode + EncodedSize>(
        &mut self,
        packet_id: i32,
        packet: &P,
    ) -> Result<()>;
}
