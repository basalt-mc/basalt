use basalt_types::{Decode, Encode, EncodedSize, VarInt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::crypto::CipherPair;
use crate::error::{Error, Result};
use crate::framing::{MAX_PACKET_SIZE, RawPacket};

/// A TCP stream with optional transparent encryption.
///
/// Wraps a `TcpStream` and an optional `CipherPair`. When encryption is
/// enabled, all reads are decrypted and all writes are encrypted
/// automatically. The caller doesn't need to know whether encryption
/// is active — the API is identical either way.
///
/// Encryption is activated once during the login handshake via
/// `enable_encryption()` and stays active for the lifetime of the
/// connection. There is no way to disable it.
pub struct EncryptedStream {
    /// The underlying TCP stream.
    stream: TcpStream,
    /// The cipher pair, if encryption has been enabled.
    cipher: Option<CipherPair>,
}

impl EncryptedStream {
    /// Creates a new unencrypted stream from a TCP connection.
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            cipher: None,
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

        // Extract packet ID
        let mut cursor = frame.as_slice();
        let packet_id = VarInt::decode(&mut cursor)
            .map_err(|e| Error::Protocol(basalt_protocol::Error::Type(e)))?;

        Ok(Some(RawPacket {
            id: packet_id.0,
            payload: cursor.to_vec(),
        }))
    }

    /// Writes a single VarInt length-prefixed packet, encrypting if needed.
    ///
    /// This is the encrypted-aware equivalent of `framing::write_raw_packet`.
    /// Builds the full frame (length + id + payload), then writes it through
    /// the encryption layer.
    pub async fn write_raw_packet(&mut self, packet_id: i32, payload: &[u8]) -> Result<()> {
        let id_varint = VarInt(packet_id);
        let frame_length = id_varint.encoded_size() + payload.len();

        let mut buf = Vec::with_capacity(VarInt(frame_length as i32).encoded_size() + frame_length);
        VarInt(frame_length as i32)
            .encode(&mut buf)
            .map_err(|e| Error::Protocol(basalt_protocol::Error::Type(e)))?;
        id_varint
            .encode(&mut buf)
            .map_err(|e| Error::Protocol(basalt_protocol::Error::Type(e)))?;
        buf.extend_from_slice(payload);

        self.write_all(&buf).await.map_err(Error::Io)
    }

    /// Writes all bytes, encrypting if encryption is active.
    ///
    /// If a cipher is active, the data is encrypted in a copy before
    /// being sent. The original data is not modified. Equivalent to
    /// `AsyncWriteExt::write_all` with transparent encryption.
    pub async fn write_all(&mut self, data: &[u8]) -> std::io::Result<()> {
        if let Some(cipher) = &mut self.cipher {
            let mut encrypted = data.to_vec();
            cipher.encrypt(&mut encrypted);
            self.stream.write_all(&encrypted).await
        } else {
            self.stream.write_all(data).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    async fn connected_pair() -> (EncryptedStream, EncryptedStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();
        (EncryptedStream::new(server), EncryptedStream::new(client))
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
    }
}
