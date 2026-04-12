use std::string::FromUtf8Error;

/// Crate-level result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during encoding or decoding of Minecraft protocol types.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("buffer underflow: need {needed} bytes, got {available}")]
    BufferUnderflow { needed: usize, available: usize },

    #[error("invalid data: {0}")]
    InvalidData(String),

    #[error("varint too large")]
    VarIntTooLarge,

    #[error("string too long: {len} bytes, max {max}")]
    StringTooLong { len: usize, max: usize },

    #[error("invalid utf-8: {0}")]
    InvalidUtf8(#[from] FromUtf8Error),

    #[error("nbt error: {0}")]
    Nbt(String),

    /// An error with added context about where it occurred during
    /// decoding (e.g., which field or packet was being decoded).
    #[error("{context}: {source}")]
    Context {
        /// Human-readable description of what was being decoded.
        context: String,
        /// The underlying error.
        source: Box<Error>,
    },
}

impl Error {
    /// Wraps this error with additional context about where it occurred.
    ///
    /// Use this to add field names, packet names, or other location
    /// information to decode errors for easier debugging.
    pub fn with_context(self, context: impl Into<String>) -> Self {
        Self::Context {
            context: context.into(),
            source: Box::new(self),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_buffer_underflow() {
        let err = Error::BufferUnderflow {
            needed: 4,
            available: 2,
        };
        assert_eq!(err.to_string(), "buffer underflow: need 4 bytes, got 2");
    }

    #[test]
    fn display_invalid_data() {
        let err = Error::InvalidData("bad value".into());
        assert_eq!(err.to_string(), "invalid data: bad value");
    }

    #[test]
    fn display_varint_too_large() {
        let err = Error::VarIntTooLarge;
        assert_eq!(err.to_string(), "varint too large");
    }

    #[test]
    fn display_string_too_long() {
        let err = Error::StringTooLong {
            len: 40000,
            max: 32767,
        };
        assert_eq!(err.to_string(), "string too long: 40000 bytes, max 32767");
    }

    #[test]
    fn display_nbt_error() {
        let err = Error::Nbt("unexpected tag type".into());
        assert_eq!(err.to_string(), "nbt error: unexpected tag type");
    }

    #[test]
    fn context_wraps_error() {
        let err = Error::BufferUnderflow {
            needed: 8,
            available: 3,
        }
        .with_context("decoding field 'x' of PlayerPosition");
        assert_eq!(
            err.to_string(),
            "decoding field 'x' of PlayerPosition: buffer underflow: need 8 bytes, got 3"
        );
    }

    #[test]
    fn from_utf8_error() {
        let invalid = vec![0xFF, 0xFE];
        let utf8_err = String::from_utf8(invalid).unwrap_err();
        let err: Error = utf8_err.into();
        assert!(matches!(err, Error::InvalidUtf8(_)));
    }
}
