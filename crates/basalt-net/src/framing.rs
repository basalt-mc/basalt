use tokio::io::{AsyncReadExt, AsyncWriteExt};

use basalt_types::{Decode, Encode, EncodedSize, VarInt};

use crate::error::{Error, Result};

/// Maximum allowed packet size in bytes (2 MiB).
///
/// Packets larger than this are rejected to prevent memory exhaustion
/// from malicious or corrupted data. This matches the vanilla Minecraft
/// server's practical limit.
pub(crate) const MAX_PACKET_SIZE: usize = 2 * 1024 * 1024;

/// A raw framed packet read from the wire.
///
/// Contains the packet ID and the payload bytes (without the length prefix
/// or the packet ID). This is the intermediate representation between the
/// TCP byte stream and the typed packet structs.
#[derive(Debug)]
pub struct RawPacket {
    /// The VarInt packet ID read from the frame.
    pub id: i32,
    /// The remaining payload bytes after the packet ID.
    pub payload: Vec<u8>,
}

/// Reads a single VarInt length-prefixed packet from an async reader.
///
/// The Minecraft protocol frames every packet as:
/// `VarInt(length) | VarInt(packet_id) | payload`
///
/// This function reads the length prefix, validates it against the max
/// packet size, reads the full frame, then splits the packet ID from
/// the payload.
///
/// Returns `None` on clean EOF (stream closed), `Err` on IO errors or
/// malformed frames.
pub async fn read_raw_packet<R: AsyncReadExt + Unpin>(reader: &mut R) -> Result<Option<RawPacket>> {
    // Read the VarInt length prefix byte-by-byte
    let length = match read_varint(reader).await? {
        Some(len) => len,
        None => return Ok(None), // Clean EOF
    };

    if length < 0 {
        return Err(Error::Protocol(basalt_mc_protocol::Error::Type(
            basalt_types::Error::InvalidData("negative packet length".into()),
        )));
    }
    let length = length as usize;

    if length > MAX_PACKET_SIZE {
        return Err(Error::PacketTooLarge {
            size: length,
            max: MAX_PACKET_SIZE,
        });
    }

    // Read the full frame (packet ID + payload)
    let mut frame = vec![0u8; length];
    reader.read_exact(&mut frame).await?;

    // Extract packet ID from the frame
    let mut cursor = frame.as_slice();
    let packet_id = basalt_types::VarInt::decode(&mut cursor)
        .map_err(|e| Error::Protocol(basalt_mc_protocol::Error::Type(e)))?;

    let payload = cursor.to_vec();

    Ok(Some(RawPacket {
        id: packet_id.0,
        payload,
    }))
}

/// Writes a single VarInt length-prefixed packet to an async writer.
///
/// Frames the packet as: `VarInt(packet_id_size + payload_size) | VarInt(packet_id) | payload`
///
/// The payload should already be encoded (the packet struct's fields as bytes).
/// The packet ID is written as a VarInt inside the frame.
pub async fn write_raw_packet<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    packet_id: i32,
    payload: &[u8],
) -> Result<()> {
    let id_varint = VarInt(packet_id);
    let frame_length = id_varint.encoded_size() + payload.len();

    // Encode the full frame: length prefix + packet ID + payload
    let mut buf = Vec::with_capacity(VarInt(frame_length as i32).encoded_size() + frame_length);
    VarInt(frame_length as i32)
        .encode(&mut buf)
        .map_err(|e| Error::Protocol(basalt_mc_protocol::Error::Type(e)))?;
    id_varint
        .encode(&mut buf)
        .map_err(|e| Error::Protocol(basalt_mc_protocol::Error::Type(e)))?;
    buf.extend_from_slice(payload);

    writer.write_all(&buf).await?;
    Ok(())
}

/// Reads a VarInt from an async reader, one byte at a time.
///
/// Returns `None` on clean EOF (first byte read returns 0 bytes).
/// Returns `Err` on IO errors or if the VarInt exceeds 5 bytes.
async fn read_varint<R: AsyncReadExt + Unpin>(reader: &mut R) -> Result<Option<i32>> {
    let mut value: u32 = 0;
    let mut position: u32 = 0;
    let mut byte = [0u8; 1];

    loop {
        match reader.read_exact(&mut byte).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                if position == 0 {
                    return Ok(None); // Clean EOF before any data
                }
                return Err(e.into()); // EOF mid-VarInt
            }
            Err(e) => return Err(e.into()),
        }

        value |= ((byte[0] & 0x7F) as u32) << position;
        position += 7;

        if byte[0] & 0x80 == 0 {
            return Ok(Some(value as i32));
        }

        if position >= 32 {
            return Err(Error::Protocol(basalt_mc_protocol::Error::Type(
                basalt_types::Error::VarIntTooLarge,
            )));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Helper: create a framed packet in a buffer (length + id + payload).
    fn frame_packet(packet_id: i32, payload: &[u8]) -> Vec<u8> {
        let id_varint = VarInt(packet_id);
        let frame_length = id_varint.encoded_size() + payload.len();

        let mut buf = Vec::new();
        VarInt(frame_length as i32).encode(&mut buf).unwrap();
        id_varint.encode(&mut buf).unwrap();
        buf.extend_from_slice(payload);
        buf
    }

    #[tokio::test]
    async fn read_empty_packet() {
        let data = frame_packet(0x00, &[]);
        let mut cursor = Cursor::new(data);
        let raw = read_raw_packet(&mut cursor).await.unwrap().unwrap();
        assert_eq!(raw.id, 0x00);
        assert!(raw.payload.is_empty());
    }

    #[tokio::test]
    async fn read_packet_with_payload() {
        let payload = [0x01, 0x02, 0x03, 0x04];
        let data = frame_packet(0x0A, &payload);
        let mut cursor = Cursor::new(data);
        let raw = read_raw_packet(&mut cursor).await.unwrap().unwrap();
        assert_eq!(raw.id, 0x0A);
        assert_eq!(raw.payload, payload);
    }

    #[tokio::test]
    async fn read_eof_returns_none() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let result = read_raw_packet(&mut cursor).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn read_oversized_packet() {
        // Frame with length > MAX_PACKET_SIZE
        let mut data = Vec::new();
        VarInt((MAX_PACKET_SIZE + 1) as i32)
            .encode(&mut data)
            .unwrap();
        let mut cursor = Cursor::new(data);
        assert!(matches!(
            read_raw_packet(&mut cursor).await,
            Err(Error::PacketTooLarge { .. })
        ));
    }

    #[tokio::test]
    async fn write_and_read_roundtrip() {
        let payload = vec![0xAA, 0xBB, 0xCC];
        let mut buf = Vec::new();
        write_raw_packet(&mut buf, 0x05, &payload).await.unwrap();

        let mut cursor = Cursor::new(buf);
        let raw = read_raw_packet(&mut cursor).await.unwrap().unwrap();
        assert_eq!(raw.id, 0x05);
        assert_eq!(raw.payload, payload);
    }

    #[tokio::test]
    async fn multiple_packets_in_stream() {
        let mut buf = Vec::new();
        write_raw_packet(&mut buf, 0x00, &[]).await.unwrap();
        write_raw_packet(&mut buf, 0x01, &[0xFF]).await.unwrap();
        write_raw_packet(&mut buf, 0x02, &[0x01, 0x02])
            .await
            .unwrap();

        let mut cursor = Cursor::new(buf);

        let p1 = read_raw_packet(&mut cursor).await.unwrap().unwrap();
        assert_eq!(p1.id, 0x00);
        assert!(p1.payload.is_empty());

        let p2 = read_raw_packet(&mut cursor).await.unwrap().unwrap();
        assert_eq!(p2.id, 0x01);
        assert_eq!(p2.payload, [0xFF]);

        let p3 = read_raw_packet(&mut cursor).await.unwrap().unwrap();
        assert_eq!(p3.id, 0x02);
        assert_eq!(p3.payload, [0x01, 0x02]);

        // EOF
        assert!(read_raw_packet(&mut cursor).await.unwrap().is_none());
    }
}
