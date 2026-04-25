//! Protocol encoding for outbound packets.
//!
//! With the SendablePacket-style taxonomy on [`ServerOutput`], the net
//! task's job here is reduced to four cases — encode (Plain), drain a
//! cached encoding (Cached), or write pre-encoded bytes (Static /
//! RawBorrowed). Domain-specific encoding logic lives at construction
//! sites in `game/`.

use basalt_net::connection::{Connection, Play};

use crate::helpers::{RawPayload, RawSlice};
use crate::messages::ServerOutput;

/// Encodes and writes a [`ServerOutput`] to the TCP connection.
///
/// All four taxonomy variants resolve to a single `write_packet_typed`
/// call after the appropriate byte source is materialised.
pub(super) async fn write_server_output(
    conn: &mut Connection<Play>,
    output: &ServerOutput,
) -> crate::error::Result<()> {
    match output {
        ServerOutput::Plain(ep) => {
            let mut data = Vec::with_capacity(ep.payload.encoded_size());
            ep.payload
                .encode(&mut data)
                .expect("packet encoding failed");
            conn.write_packet_typed(ep.id, &RawPayload(data)).await?;
        }
        ServerOutput::Cached(shared) => {
            let packets = shared.get_or_encode();
            for (id, data) in packets {
                conn.write_packet_typed(*id, &RawSlice(data)).await?;
            }
        }
        ServerOutput::Static { id, bytes } => {
            conn.write_packet_typed(*id, &RawSlice(bytes)).await?;
        }
        ServerOutput::RawBorrowed { id, bytes } => {
            conn.write_packet_typed(*id, &RawSlice(bytes)).await?;
        }
    }
    Ok(())
}
