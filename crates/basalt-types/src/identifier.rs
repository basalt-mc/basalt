use std::fmt;

use crate::{Decode, Encode, EncodedSize, Error, Result, VarInt};

/// A namespaced identifier in the format `namespace:path`.
///
/// Identifiers (also called ResourceLocations) are used throughout the
/// Minecraft protocol to reference game content: blocks (`minecraft:stone`),
/// items (`minecraft:diamond`), entities (`minecraft:creeper`), dimensions
/// (`minecraft:overworld`), registries, and plugin channels. They are
/// encoded on the wire as a single VarInt-prefixed UTF-8 string in the
/// format `namespace:path`.
///
/// The namespace defaults to `minecraft` when absent. Valid characters are:
/// - Namespace: `[a-z0-9._-]`
/// - Path: `[a-z0-9._-/]`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identifier {
    /// The namespace part (e.g., `minecraft`, `mymod`).
    pub namespace: String,
    /// The path part (e.g., `stone`, `textures/block/dirt`).
    pub path: String,
}

impl Identifier {
    /// Creates a new identifier with the given namespace and path.
    ///
    /// Validates that both namespace and path contain only allowed characters.
    /// Returns `Error::InvalidData` if validation fails.
    pub fn new(namespace: impl Into<String>, path: impl Into<String>) -> Result<Self> {
        let namespace = namespace.into();
        let path = path.into();

        if !namespace.chars().all(is_valid_namespace_char) {
            return Err(Error::InvalidData(format!(
                "invalid namespace character in '{namespace}'"
            )));
        }
        if !path.chars().all(is_valid_path_char) {
            return Err(Error::InvalidData(format!(
                "invalid path character in '{path}'"
            )));
        }

        Ok(Self { namespace, path })
    }

    /// Creates a new identifier under the `minecraft` namespace.
    ///
    /// This is a convenience for the most common case, since the majority
    /// of identifiers in the protocol use the `minecraft` namespace.
    pub fn minecraft(path: impl Into<String>) -> Result<Self> {
        Self::new("minecraft", path)
    }

    /// Returns the full identifier string in `namespace:path` format.
    pub fn as_str(&self) -> String {
        format!("{}:{}", self.namespace, self.path)
    }
}

/// Returns true if the character is valid in an identifier namespace.
///
/// Allowed characters: lowercase ASCII letters, digits, dots, underscores,
/// and hyphens (`[a-z0-9._-]`).
fn is_valid_namespace_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' || c == '-'
}

/// Returns true if the character is valid in an identifier path.
///
/// Allowed characters: same as namespace plus forward slashes
/// (`[a-z0-9._-/]`). Slashes enable hierarchical paths like
/// `textures/block/dirt`.
fn is_valid_path_char(c: char) -> bool {
    is_valid_namespace_char(c) || c == '/'
}

/// Parses an identifier string in `namespace:path` or bare `path` format.
///
/// If no colon is present, the namespace defaults to `minecraft`, matching
/// the Minecraft protocol convention. Returns `Error::InvalidData` if the
/// string contains invalid characters or is empty.
fn parse_identifier(s: &str) -> Result<Identifier> {
    if s.is_empty() {
        return Err(Error::InvalidData("empty identifier".into()));
    }

    let (namespace, path) = match s.find(':') {
        Some(pos) => (&s[..pos], &s[pos + 1..]),
        None => ("minecraft", s),
    };

    Identifier::new(namespace, path)
}

/// Encodes an Identifier as a VarInt-prefixed UTF-8 string in `namespace:path` format.
///
/// The full `namespace:path` string is written using the standard Minecraft
/// string encoding (VarInt length prefix + UTF-8 bytes). This is the same
/// wire format used for all string fields in the protocol.
impl Encode for Identifier {
    /// Writes the identifier as a VarInt-prefixed `namespace:path` string.
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        self.as_str().encode(buf)
    }
}

/// Decodes an Identifier from a VarInt-prefixed UTF-8 string.
///
/// Reads the string using the standard Minecraft string decoding, then
/// parses it as `namespace:path`. If no colon is present, the namespace
/// defaults to `minecraft`. Validates that all characters are in the
/// allowed sets for namespace and path.
impl Decode for Identifier {
    /// Reads a protocol string and parses it as an identifier.
    ///
    /// Fails with `Error::InvalidData` if the identifier contains invalid
    /// characters or is empty. Also inherits string decoding errors
    /// (buffer underflow, string too long, invalid UTF-8).
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        let s = String::decode(buf)?;
        parse_identifier(&s)
    }
}

/// Computes the wire size of the identifier in `namespace:path` format.
///
/// The total size includes the VarInt length prefix and the full
/// `namespace:path` UTF-8 byte count (including the colon separator).
impl EncodedSize for Identifier {
    /// Returns the VarInt prefix size plus the byte length of `namespace:path`.
    fn encoded_size(&self) -> usize {
        let str_len = self.namespace.len() + 1 + self.path.len();
        VarInt(str_len as i32).encoded_size() + str_len
    }
}

/// Displays the identifier in `namespace:path` format.
impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(namespace: &str, path: &str) {
        let id = Identifier::new(namespace, path).unwrap();
        let mut buf = Vec::with_capacity(id.encoded_size());
        id.encode(&mut buf).unwrap();
        assert_eq!(buf.len(), id.encoded_size());

        let mut cursor = buf.as_slice();
        let decoded = Identifier::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, id);
    }

    // -- Construction --

    #[test]
    fn new_valid() {
        let id = Identifier::new("minecraft", "stone").unwrap();
        assert_eq!(id.namespace, "minecraft");
        assert_eq!(id.path, "stone");
    }

    #[test]
    fn minecraft_shorthand() {
        let id = Identifier::minecraft("diamond").unwrap();
        assert_eq!(id.namespace, "minecraft");
        assert_eq!(id.path, "diamond");
    }

    #[test]
    fn custom_namespace() {
        let id = Identifier::new("mymod", "custom_block").unwrap();
        assert_eq!(id.namespace, "mymod");
        assert_eq!(id.path, "custom_block");
    }

    #[test]
    fn path_with_slashes() {
        let id = Identifier::new("minecraft", "textures/block/dirt").unwrap();
        assert_eq!(id.path, "textures/block/dirt");
    }

    #[test]
    fn invalid_namespace_uppercase() {
        assert!(Identifier::new("Minecraft", "stone").is_err());
    }

    #[test]
    fn invalid_namespace_space() {
        assert!(Identifier::new("my mod", "stone").is_err());
    }

    #[test]
    fn invalid_path_uppercase() {
        assert!(Identifier::new("minecraft", "Stone").is_err());
    }

    #[test]
    fn valid_special_chars() {
        assert!(Identifier::new("my-mod.v2", "custom_item-v3").is_ok());
    }

    // -- Parsing --

    #[test]
    fn parse_with_namespace() {
        let id = parse_identifier("minecraft:stone").unwrap();
        assert_eq!(id.namespace, "minecraft");
        assert_eq!(id.path, "stone");
    }

    #[test]
    fn parse_without_namespace() {
        let id = parse_identifier("stone").unwrap();
        assert_eq!(id.namespace, "minecraft");
        assert_eq!(id.path, "stone");
    }

    #[test]
    fn parse_empty() {
        assert!(parse_identifier("").is_err());
    }

    #[test]
    fn parse_custom_namespace() {
        let id = parse_identifier("mymod:custom_block").unwrap();
        assert_eq!(id.namespace, "mymod");
        assert_eq!(id.path, "custom_block");
    }

    // -- Encode/Decode --

    #[test]
    fn roundtrip_minecraft() {
        roundtrip("minecraft", "stone");
    }

    #[test]
    fn roundtrip_custom() {
        roundtrip("mymod", "custom_block");
    }

    #[test]
    fn roundtrip_with_path_slashes() {
        roundtrip("minecraft", "textures/block/dirt");
    }

    #[test]
    fn decode_bare_path() {
        // Encode "stone" (no namespace) as a raw string
        let mut buf = Vec::new();
        "stone".to_string().encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let id = Identifier::decode(&mut cursor).unwrap();
        assert_eq!(id.namespace, "minecraft");
        assert_eq!(id.path, "stone");
    }

    // -- Display --

    #[test]
    fn display() {
        let id = Identifier::new("minecraft", "stone").unwrap();
        assert_eq!(id.to_string(), "minecraft:stone");
    }

    #[test]
    fn as_str() {
        let id = Identifier::new("mymod", "item").unwrap();
        assert_eq!(id.as_str(), "mymod:item");
    }

    // -- EncodedSize --

    #[test]
    fn encoded_size_includes_colon() {
        let id = Identifier::new("minecraft", "stone").unwrap();
        // "minecraft:stone" = 15 chars, VarInt(15) = 1 byte
        assert_eq!(id.encoded_size(), 16);
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        fn namespace_strategy() -> impl Strategy<Value = String> {
            "[a-z0-9._\\-]{1,20}"
        }

        fn path_strategy() -> impl Strategy<Value = String> {
            "[a-z0-9._\\-/]{1,50}"
        }

        proptest! {
            #[test]
            fn identifier_roundtrip(
                namespace in namespace_strategy(),
                path in path_strategy(),
            ) {
                roundtrip(&namespace, &path);
            }
        }
    }
}
