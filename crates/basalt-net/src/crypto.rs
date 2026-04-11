use aes::cipher::{BlockEncrypt, KeyInit};
use aes::{Aes128, Block};

/// AES-128 CFB-8 encryption/decryption for Minecraft protocol traffic.
///
/// The Minecraft protocol uses AES in CFB-8 mode (8-bit feedback) for
/// encrypting all traffic after the login handshake. CFB-8 processes one
/// byte at a time, which allows encryption/decryption without padding or
/// block alignment — essential for a streaming protocol.
///
/// The same 16-byte shared secret is used as both the key and the IV
/// (initialization vector), as specified by the Minecraft protocol. Read
/// and write directions maintain independent cipher states.
///
/// This is a manual CFB-8 implementation using raw AES block encryption,
/// because the cfb8 crate's API consumes the cipher on each operation,
/// making it unsuitable for a stateful streaming context.
pub struct CipherPair {
    /// The AES-128 cipher, shared between encrypt and decrypt.
    cipher: Aes128,
    /// The encryption shift register (16 bytes, updated after each byte).
    enc_iv: [u8; 16],
    /// The decryption shift register (16 bytes, updated after each byte).
    dec_iv: [u8; 16],
}

impl CipherPair {
    /// Creates a new cipher pair from a 16-byte shared secret.
    ///
    /// The shared secret is used as both the AES key and the initial IV
    /// for both directions, as specified by the Minecraft protocol.
    /// This is established during the login handshake via RSA key exchange.
    pub fn new(shared_secret: &[u8; 16]) -> Self {
        Self {
            cipher: Aes128::new(shared_secret.into()),
            enc_iv: *shared_secret,
            dec_iv: *shared_secret,
        }
    }

    /// Encrypts data in place using CFB-8 mode.
    ///
    /// Processes one byte at a time: encrypts the IV with AES, XORs the
    /// first byte of the result with the plaintext byte, then shifts the
    /// IV left by one byte and appends the ciphertext byte. This maintains
    /// the streaming cipher state across calls.
    pub fn encrypt(&mut self, data: &mut [u8]) {
        for byte in data.iter_mut() {
            let mut block = Block::from(self.enc_iv);
            self.cipher.encrypt_block(&mut block);

            *byte ^= block[0];

            // Shift IV left by 1 byte, append ciphertext byte
            self.enc_iv.copy_within(1.., 0);
            self.enc_iv[15] = *byte;
        }
    }

    /// Decrypts data in place using CFB-8 mode.
    ///
    /// The inverse of encrypt: encrypts the IV with AES (same as encrypt),
    /// saves the ciphertext byte, XORs with the AES output to recover
    /// plaintext, then shifts the IV with the original ciphertext byte.
    pub fn decrypt(&mut self, data: &mut [u8]) {
        for byte in data.iter_mut() {
            let mut block = Block::from(self.dec_iv);
            self.cipher.encrypt_block(&mut block);

            let ciphertext = *byte;
            *byte ^= block[0];

            // Shift IV left by 1 byte, append ciphertext byte
            self.dec_iv.copy_within(1.., 0);
            self.dec_iv[15] = ciphertext;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let secret = [0x42u8; 16];
        let mut encryptor = CipherPair::new(&secret);
        let mut decryptor = CipherPair::new(&secret);

        let original = b"Hello, Minecraft!".to_vec();
        let mut data = original.clone();

        encryptor.encrypt(&mut data);
        assert_ne!(
            data, original,
            "encrypted data should differ from plaintext"
        );

        decryptor.decrypt(&mut data);
        assert_eq!(data, original, "decrypted data should match original");
    }

    #[test]
    fn stateful_cipher() {
        let secret = [0xAB; 16];
        let mut enc = CipherPair::new(&secret);
        let mut dec = CipherPair::new(&secret);

        // Encrypt two chunks separately — cipher state carries over
        let mut chunk1 = b"first".to_vec();
        let mut chunk2 = b"second".to_vec();
        enc.encrypt(&mut chunk1);
        enc.encrypt(&mut chunk2);

        dec.decrypt(&mut chunk1);
        dec.decrypt(&mut chunk2);
        assert_eq!(&chunk1, b"first");
        assert_eq!(&chunk2, b"second");
    }

    #[test]
    fn single_byte_at_a_time() {
        let secret = [0x01; 16];
        let mut enc = CipherPair::new(&secret);
        let mut dec = CipherPair::new(&secret);

        // CFB-8 supports byte-by-byte operation
        let original = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let mut data = original.clone();

        for byte in data.iter_mut() {
            enc.encrypt(std::slice::from_mut(byte));
        }

        for byte in data.iter_mut() {
            dec.decrypt(std::slice::from_mut(byte));
        }

        assert_eq!(data, original);
    }

    #[test]
    fn empty_data() {
        let secret = [0x00; 16];
        let mut cipher = CipherPair::new(&secret);
        let mut data = vec![];
        cipher.encrypt(&mut data);
        cipher.decrypt(&mut data);
        assert!(data.is_empty());
    }

    #[test]
    fn large_data() {
        let secret = [0x77; 16];
        let mut enc = CipherPair::new(&secret);
        let mut dec = CipherPair::new(&secret);

        let original: Vec<u8> = (0..4096).map(|i| (i % 256) as u8).collect();
        let mut data = original.clone();

        enc.encrypt(&mut data);
        assert_ne!(data, original);

        dec.decrypt(&mut data);
        assert_eq!(data, original);
    }
}
