use basalt_types::{Decode, Encode, VarInt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::compression;
use crate::crypto::CipherPair;
use crate::error::{Error, Result};
use crate::framing::{MAX_PACKET_SIZE, RawPacket};

/// A TCP stream with optional transparent encryption and compression.
///
/// Wraps a `TcpStream` with optional AES-128 CFB-8 encryption and zlib
/// compression. Both layers are transparent to the caller — the API is
/// identical regardless of which layers are active.
///
/// The layers are activated during the login handshake:
/// 1. Encryption via `enable_encryption()` after Encryption Response
/// 2. Compression via `enable_compression()` after Set Compression
///
/// Once enabled, neither layer can be disabled. On the wire, compression
/// is applied first (at the frame level), then encryption wraps everything
/// including the length prefix.
pub struct ProtocolStream<W = TcpStream> {
    /// The underlying byte stream. Defaults to `TcpStream` so existing
    /// call sites (`Connection::accept`, etc.) compile unchanged; tests
    /// and benches can substitute any `AsyncRead + AsyncWrite + Unpin`
    /// type (e.g. `tokio::io::DuplexStream`).
    stream: W,
    /// The cipher pair, if encryption has been enabled.
    cipher: Option<CipherPair>,
    /// The compression threshold in bytes, if compression has been enabled.
    /// Packets with uncompressed size >= threshold are zlib-compressed.
    /// `None` means compression is disabled.
    compression_threshold: Option<usize>,
    /// Reusable buffer for encrypting outgoing data, avoiding per-write allocation.
    encrypt_buf: Vec<u8>,
    /// Reusable staging buffer for the uncompressed packet body
    /// (`VarInt(packet_id)` + payload). Cleared and reused on every
    /// `write_raw_packet` call, eliminating the per-packet `Vec`
    /// allocation that dominated the broadcast hot path (#175).
    packet_buf: Vec<u8>,
    /// Reusable staging buffer for the zlib-compressed frame content.
    /// Only populated when compression is active; otherwise stays empty
    /// at zero capacity.
    compressed_buf: Vec<u8>,
    /// Reusable staging buffer for the framed wire bytes
    /// (`VarInt(frame_length)` + frame content). Same lifecycle as
    /// `packet_buf`.
    frame_buf: Vec<u8>,
}

impl<W> ProtocolStream<W> {
    /// Creates a new unencrypted stream from any byte-stream backing
    /// (`TcpStream` in production, `DuplexStream` in benches/tests).
    pub fn new(stream: W) -> Self {
        Self {
            stream,
            cipher: None,
            compression_threshold: None,
            encrypt_buf: Vec::new(),
            packet_buf: Vec::new(),
            compressed_buf: Vec::new(),
            frame_buf: Vec::new(),
        }
    }

    /// Enables AES-128 CFB-8 encryption on this stream.
    ///
    /// All subsequent reads will be decrypted and all writes encrypted
    /// using the provided shared secret. The shared secret is used as
    /// both the AES key and the CFB-8 IV, as specified by the Minecraft
    /// protocol.
    ///
    /// This should be called after receiving the client's Encryption
    /// Response packet during the login handshake. Once enabled,
    /// encryption cannot be disabled.
    pub fn enable_encryption(&mut self, shared_secret: &[u8; 16]) {
        self.cipher = Some(CipherPair::new(shared_secret));
    }

    /// Returns true if encryption is currently active.
    pub fn is_encrypted(&self) -> bool {
        self.cipher.is_some()
    }

    /// Enables zlib compression on this stream.
    ///
    /// Packets with uncompressed size >= threshold bytes will be
    /// zlib-compressed. Packets below threshold are sent uncompressed
    /// with a data_length of 0. This should be called after sending
    /// the Set Compression packet during login/configuration.
    pub fn enable_compression(&mut self, threshold: usize) {
        self.compression_threshold = Some(threshold);
    }

    /// Returns true if compression is currently active.
    pub fn is_compressed(&self) -> bool {
        self.compression_threshold.is_some()
    }
}

/// Async I/O methods — require the underlying stream to support both
/// reading and writing. The bidirectional bound matches how
/// `ProtocolStream` is actually used (handshake exchanges in both
/// directions on the same stream).
impl<W: AsyncReadExt + AsyncWriteExt + Unpin> ProtocolStream<W> {
    /// Reads exactly `buf.len()` bytes, decrypting if encryption is active.
    ///
    /// Reads raw bytes from the TCP stream, then decrypts them in place
    /// if a cipher is active. Equivalent to `AsyncReadExt::read_exact`
    /// with transparent decryption.
    pub async fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.stream.read_exact(buf).await?;
        if let Some(cipher) = &mut self.cipher {
            cipher.decrypt(buf);
        }
        Ok(n)
    }

    /// Reads a single VarInt length-prefixed packet, decrypting if needed.
    ///
    /// This is the encrypted-aware equivalent of `framing::read_raw_packet`.
    /// Reads the VarInt length byte-by-byte (each byte decrypted individually
    /// in CFB-8 mode), then reads the full frame and decrypts it.
    pub async fn read_raw_packet(&mut self) -> Result<Option<RawPacket>> {
        // Read VarInt length prefix byte-by-byte
        let mut value: u32 = 0;
        let mut position: u32 = 0;
        let mut byte = [0u8; 1];

        let length = loop {
            match self.read_exact(&mut byte).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    if position == 0 {
                        return Ok(None);
                    }
                    return Err(Error::Io(e));
                }
                Err(e) => return Err(Error::Io(e)),
            }

            value |= ((byte[0] & 0x7F) as u32) << position;
            position += 7;

            if byte[0] & 0x80 == 0 {
                break value as i32;
            }

            if position >= 32 {
                return Err(Error::Protocol(basalt_protocol::Error::Type(
                    basalt_types::Error::VarIntTooLarge,
                )));
            }
        };

        if length < 0 {
            return Err(Error::Protocol(basalt_protocol::Error::Type(
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

        // Read full frame (decrypted transparently)
        let mut frame = vec![0u8; length];
        self.read_exact(&mut frame).await.map_err(Error::Io)?;

        // Decompress if compression is enabled
        let data = if self.compression_threshold.is_some() {
            compression::decompress_packet(&frame)?
        } else {
            frame
        };

        // Extract packet ID
        let mut cursor = data.as_slice();
        let packet_id = VarInt::decode(&mut cursor)
            .map_err(|e| Error::Protocol(basalt_protocol::Error::Type(e)))?;

        Ok(Some(RawPacket {
            id: packet_id.0,
            payload: cursor.to_vec(),
        }))
    }

    /// Writes a single VarInt length-prefixed packet, with optional
    /// compression and encryption.
    ///
    /// When compression is enabled, the packet ID + payload are compressed
    /// if they exceed the threshold. The compressed (or uncompressed) data
    /// is then framed with a VarInt length prefix and written through the
    /// encryption layer.
    ///
    /// All staging is done in `self.packet_buf` / `self.compressed_buf` /
    /// `self.frame_buf` (cleared then reused on every call) so the hot
    /// broadcast path doesn't allocate.
    pub async fn write_raw_packet(&mut self, packet_id: i32, payload: &[u8]) -> Result<()> {
        let id_varint = VarInt(packet_id);

        // Stage 1: id + payload → packet_buf
        self.packet_buf.clear();
        id_varint
            .encode(&mut self.packet_buf)
            .map_err(|e| Error::Protocol(basalt_protocol::Error::Type(e)))?;
        self.packet_buf.extend_from_slice(payload);

        // Stage 2: optional compression → compressed_buf. Borrowing
        // `self.packet_buf` as `&[u8]` resolves the input borrow before
        // the `&mut self.compressed_buf` is taken, so the two field
        // accesses don't conflict.
        let frame_content: &[u8] = if let Some(threshold) = self.compression_threshold {
            let input: &[u8] = &self.packet_buf;
            compression::compress_packet_into(input, threshold, &mut self.compressed_buf)?;
            &self.compressed_buf
        } else {
            &self.packet_buf
        };

        // Stage 3: length prefix + content → frame_buf
        self.frame_buf.clear();
        VarInt(frame_content.len() as i32)
            .encode(&mut self.frame_buf)
            .map_err(|e| Error::Protocol(basalt_protocol::Error::Type(e)))?;
        self.frame_buf.extend_from_slice(frame_content);

        // Stage 4: encrypted write — delegates to a private helper that
        // owns `&mut self`, so the borrow on `self.frame_buf` doesn't
        // conflict at the call site.
        self.write_buffered().await.map_err(Error::Io)
    }

    /// Writes the contents of `self.frame_buf` to the wire, encrypting
    /// through `self.encrypt_buf` if a cipher is active.
    ///
    /// Private helper extracted from `write_raw_packet` Stage 4 — kept
    /// inside `&mut self` so the cipher / encrypt_buf / frame_buf /
    /// stream borrows resolve as split borrows on disjoint fields.
    /// Mirrors the dispatch in [`Self::write_all`] but operates on the
    /// stream-owned buffer rather than an arbitrary `&[u8]`.
    async fn write_buffered(&mut self) -> std::io::Result<()> {
        if let Some(cipher) = &mut self.cipher {
            self.encrypt_buf.clear();
            self.encrypt_buf.extend_from_slice(&self.frame_buf);
            cipher.encrypt(&mut self.encrypt_buf);
            self.stream.write_all(&self.encrypt_buf).await
        } else {
            self.stream.write_all(&self.frame_buf).await
        }
    }

    /// Writes all bytes, encrypting if encryption is active.
    ///
    /// If a cipher is active, the data is encrypted in a copy before
    /// being sent. The original data is not modified. Equivalent to
    /// `AsyncWriteExt::write_all` with transparent encryption.
    pub async fn write_all(&mut self, data: &[u8]) -> std::io::Result<()> {
        if let Some(cipher) = &mut self.cipher {
            self.encrypt_buf.clear();
            self.encrypt_buf.extend_from_slice(data);
            cipher.encrypt(&mut self.encrypt_buf);
            self.stream.write_all(&self.encrypt_buf).await
        } else {
            self.stream.write_all(data).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    async fn connected_pair() -> (ProtocolStream, ProtocolStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();
        (ProtocolStream::new(server), ProtocolStream::new(client))
    }

    #[tokio::test]
    async fn unencrypted_roundtrip() {
        let (mut server, mut client) = connected_pair().await;

        client.write_all(b"hello").await.unwrap();
        let mut buf = [0u8; 5];
        server.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");
    }

    #[tokio::test]
    async fn encrypted_roundtrip() {
        let (mut server, mut client) = connected_pair().await;

        let secret = [0x42u8; 16];
        server.enable_encryption(&secret);
        client.enable_encryption(&secret);

        assert!(server.is_encrypted());
        assert!(client.is_encrypted());

        // Client writes encrypted, server reads and decrypts
        client.write_all(b"secret message").await.unwrap();
        let mut buf = [0u8; 14];
        server.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"secret message");
    }

    #[tokio::test]
    async fn encrypted_bidirectional() {
        let (mut server, mut client) = connected_pair().await;

        let secret = [0xAB; 16];
        server.enable_encryption(&secret);
        client.enable_encryption(&secret);

        // Client → Server
        client.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        server.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");

        // Server → Client
        server.write_all(b"pong").await.unwrap();
        let mut buf = [0u8; 4];
        client.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"pong");
    }

    #[tokio::test]
    async fn encrypted_multiple_chunks() {
        let (mut server, mut client) = connected_pair().await;

        let secret = [0x01; 16];
        server.enable_encryption(&secret);
        client.enable_encryption(&secret);

        // Send multiple chunks — cipher state must be consistent
        for i in 0..10u8 {
            client.write_all(&[i; 32]).await.unwrap();
        }

        for i in 0..10u8 {
            let mut buf = [0u8; 32];
            server.read_exact(&mut buf).await.unwrap();
            assert_eq!(buf, [i; 32]);
        }
    }

    #[tokio::test]
    async fn not_encrypted_by_default() {
        let (server, client) = connected_pair().await;
        assert!(!server.is_encrypted());
        assert!(!client.is_encrypted());
        assert!(!server.is_compressed());
        assert!(!client.is_compressed());
    }

    #[tokio::test]
    async fn compressed_packet_roundtrip() {
        let (mut server, mut client) = connected_pair().await;

        server.enable_compression(256);
        client.enable_compression(256);

        // Small packet — below threshold, not compressed
        let payload = vec![0x01, 0x02, 0x03];
        client.write_raw_packet(0x00, &payload).await.unwrap();
        let raw = server.read_raw_packet().await.unwrap().unwrap();
        assert_eq!(raw.id, 0x00);
        assert_eq!(raw.payload, payload);
    }

    #[tokio::test]
    async fn compressed_large_packet_roundtrip() {
        let (mut server, mut client) = connected_pair().await;

        server.enable_compression(256);
        client.enable_compression(256);

        // Large packet — above threshold, should be compressed
        let payload = vec![0xAB; 1024];
        client.write_raw_packet(0x05, &payload).await.unwrap();
        let raw = server.read_raw_packet().await.unwrap().unwrap();
        assert_eq!(raw.id, 0x05);
        assert_eq!(raw.payload, payload);
    }

    #[tokio::test]
    async fn encrypted_and_compressed_roundtrip() {
        let (mut server, mut client) = connected_pair().await;

        let secret = [0x77; 16];
        server.enable_encryption(&secret);
        client.enable_encryption(&secret);
        server.enable_compression(128);
        client.enable_compression(128);

        // Small packet (below compression threshold)
        let small = vec![0x01; 10];
        client.write_raw_packet(0x00, &small).await.unwrap();
        let raw = server.read_raw_packet().await.unwrap().unwrap();
        assert_eq!(raw.id, 0x00);
        assert_eq!(raw.payload, small);

        // Large packet (above compression threshold)
        let large = vec![0xFF; 512];
        client.write_raw_packet(0x01, &large).await.unwrap();
        let raw = server.read_raw_packet().await.unwrap().unwrap();
        assert_eq!(raw.id, 0x01);
        assert_eq!(raw.payload, large);
    }
}
