use std::fmt;

/// NBT tag type IDs as defined by the Minecraft protocol specification.
///
/// Each NBT tag is identified by a single byte indicating its type.
/// These constants match the official Minecraft NBT format specification
/// and are used during encoding/decoding to identify tag payloads.
pub mod tag_id {
    pub const END: u8 = 0;
    pub const BYTE: u8 = 1;
    pub const SHORT: u8 = 2;
    pub const INT: u8 = 3;
    pub const LONG: u8 = 4;
    pub const FLOAT: u8 = 5;
    pub const DOUBLE: u8 = 6;
    pub const BYTE_ARRAY: u8 = 7;
    pub const STRING: u8 = 8;
    pub const LIST: u8 = 9;
    pub const COMPOUND: u8 = 10;
    pub const INT_ARRAY: u8 = 11;
    pub const LONG_ARRAY: u8 = 12;
}

/// A single NBT value of any supported type.
///
/// NBT (Named Binary Tag) is Minecraft's hierarchical binary data format.
/// Each tag holds a typed value and is identified by a tag type byte. In
/// compounds, tags are named; in lists, tags are ordered and homogeneous.
///
/// This enum covers all 12 NBT tag types used in the Minecraft protocol.
/// The `End` tag (type 0) is not represented as a value — it only appears
/// as a sentinel marking the end of a compound in the wire format.
#[derive(Debug, Clone, PartialEq)]
pub enum NbtTag {
    /// A single signed byte (tag type 1).
    /// Used for boolean flags, small counters, and game mode values.
    Byte(i8),

    /// A 16-bit signed integer (tag type 2).
    /// Used for enchantment levels, item durability, and similar values.
    Short(i16),

    /// A 32-bit signed integer (tag type 3).
    /// Used for entity IDs, scores, and general-purpose integer data.
    Int(i32),

    /// A 64-bit signed integer (tag type 4).
    /// Used for world time, UUID components, and large numeric values.
    Long(i64),

    /// A 32-bit IEEE 754 float (tag type 5).
    /// Used for entity rotation, knockback strength, and similar values.
    Float(f32),

    /// A 64-bit IEEE 754 double (tag type 6).
    /// Used for entity positions, world coordinates, and precise values.
    Double(f64),

    /// A length-prefixed array of signed bytes (tag type 7).
    /// Length is encoded as a big-endian i32 (not VarInt).
    /// Used for block data, heightmaps, and biome data.
    ByteArray(Vec<i8>),

    /// A modified UTF-8 string (tag type 8).
    /// Length is encoded as a big-endian u16 (not VarInt).
    /// Used for tag names, text values, JSON payloads, and identifiers.
    String(String),

    /// An ordered list of tags, all of the same type (tag type 9).
    /// Encoded as: tag type byte + i32 length + payloads (no names).
    /// Used for lists of items, entities, coordinates, and nested data.
    List(NbtList),

    /// An unordered collection of named tags (tag type 10).
    /// Each entry is a tag type byte + name + payload, terminated by
    /// an End tag (type 0). Used for structured data like items, entities,
    /// and block entities.
    Compound(NbtCompound),

    /// A length-prefixed array of 32-bit signed integers (tag type 11).
    /// Length is encoded as a big-endian i32.
    /// Used for heightmaps and biome data in newer formats.
    IntArray(Vec<i32>),

    /// A length-prefixed array of 64-bit signed integers (tag type 12).
    /// Length is encoded as a big-endian i32.
    /// Used for packed heightmaps and block states in chunk data.
    LongArray(Vec<i64>),
}

impl NbtTag {
    /// Returns the NBT tag type ID for this value.
    ///
    /// This is the single-byte identifier written before the tag payload
    /// in the NBT wire format. Used during encoding to write the correct
    /// type byte for compound entries and list element types.
    pub fn tag_id(&self) -> u8 {
        match self {
            NbtTag::Byte(_) => tag_id::BYTE,
            NbtTag::Short(_) => tag_id::SHORT,
            NbtTag::Int(_) => tag_id::INT,
            NbtTag::Long(_) => tag_id::LONG,
            NbtTag::Float(_) => tag_id::FLOAT,
            NbtTag::Double(_) => tag_id::DOUBLE,
            NbtTag::ByteArray(_) => tag_id::BYTE_ARRAY,
            NbtTag::String(_) => tag_id::STRING,
            NbtTag::List(_) => tag_id::LIST,
            NbtTag::Compound(_) => tag_id::COMPOUND,
            NbtTag::IntArray(_) => tag_id::INT_ARRAY,
            NbtTag::LongArray(_) => tag_id::LONG_ARRAY,
        }
    }
}

/// A homogeneous ordered list of NBT tags.
///
/// All elements in an NBT list must have the same tag type. The wire format
/// encodes the element type once (as a single byte), followed by the count
/// (as a big-endian i32), then each element's payload without type bytes
/// or names. An empty list uses `End` (0) as the element type.
#[derive(Debug, Clone, PartialEq)]
pub struct NbtList {
    /// The tag type ID shared by all elements.
    /// For empty lists, this is `tag_id::END` (0).
    pub element_type: u8,

    /// The list elements. All must have a `tag_id()` matching `element_type`,
    /// or `element_type` must be `tag_id::END` when the list is empty.
    pub elements: Vec<NbtTag>,
}

impl NbtList {
    /// Creates a new empty NBT list.
    ///
    /// The element type is set to `End` (0), which is the standard
    /// encoding for empty lists in the NBT format.
    pub fn new() -> Self {
        Self {
            element_type: tag_id::END,
            elements: Vec::new(),
        }
    }

    /// Creates an NBT list from a vector of tags.
    ///
    /// All tags must have the same type. The element type is inferred
    /// from the first element. Returns `None` if the tags have mixed types.
    /// Returns an empty list with `End` element type if the vector is empty.
    pub fn from_tags(tags: Vec<NbtTag>) -> Option<Self> {
        if tags.is_empty() {
            return Some(Self::new());
        }

        let element_type = tags[0].tag_id();
        if tags.iter().all(|t| t.tag_id() == element_type) {
            Some(Self {
                element_type,
                elements: tags,
            })
        } else {
            None
        }
    }

    /// Returns the number of elements in the list.
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Returns true if the list contains no elements.
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }
}

impl Default for NbtList {
    fn default() -> Self {
        Self::new()
    }
}

/// An unordered collection of named NBT tags.
///
/// NbtCompound is the primary structured data container in NBT. It maps
/// string names to typed values and is used for virtually all complex
/// game data: items, entities, block entities, player data, level data.
///
/// The wire format encodes each entry as: tag type byte + name (u16-prefixed
/// UTF-8) + payload, terminated by an End tag (type 0, no name, no payload).
///
/// Internally uses a `Vec` of `(String, NbtTag)` pairs to preserve
/// insertion order, which matters for consistent encoding.
#[derive(Debug, Clone, PartialEq)]
pub struct NbtCompound {
    entries: Vec<(String, NbtTag)>,
}

impl NbtCompound {
    /// Creates a new empty compound.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Inserts a named tag into the compound.
    ///
    /// If a tag with the same name already exists, it is replaced.
    /// The insertion order of new keys is preserved.
    pub fn insert(&mut self, name: impl Into<String>, tag: NbtTag) {
        let name = name.into();
        if let Some(entry) = self.entries.iter_mut().find(|(n, _)| *n == name) {
            entry.1 = tag;
        } else {
            self.entries.push((name, tag));
        }
    }

    /// Returns a reference to the tag with the given name, if it exists.
    pub fn get(&self, name: &str) -> Option<&NbtTag> {
        self.entries.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }

    /// Returns an iterator over all `(name, tag)` pairs in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &NbtTag)> {
        self.entries.iter().map(|(n, t)| (n.as_str(), t))
    }

    /// Returns the number of entries in the compound.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the compound contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns true if the compound contains a tag with the given name.
    pub fn contains_key(&self, name: &str) -> bool {
        self.entries.iter().any(|(n, _)| n == name)
    }
}

impl Default for NbtCompound {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for NbtTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NbtTag::Byte(v) => write!(f, "{v}b"),
            NbtTag::Short(v) => write!(f, "{v}s"),
            NbtTag::Int(v) => write!(f, "{v}"),
            NbtTag::Long(v) => write!(f, "{v}L"),
            NbtTag::Float(v) => write!(f, "{v}f"),
            NbtTag::Double(v) => write!(f, "{v}d"),
            NbtTag::ByteArray(v) => write!(f, "[B; {} bytes]", v.len()),
            NbtTag::String(v) => write!(f, "\"{v}\""),
            NbtTag::List(v) => write!(f, "[{} entries]", v.len()),
            NbtTag::Compound(v) => write!(f, "{{{} entries}}", v.len()),
            NbtTag::IntArray(v) => write!(f, "[I; {} ints]", v.len()),
            NbtTag::LongArray(v) => write!(f, "[L; {} longs]", v.len()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- NbtTag --

    #[test]
    fn tag_ids() {
        assert_eq!(NbtTag::Byte(0).tag_id(), tag_id::BYTE);
        assert_eq!(NbtTag::Short(0).tag_id(), tag_id::SHORT);
        assert_eq!(NbtTag::Int(0).tag_id(), tag_id::INT);
        assert_eq!(NbtTag::Long(0).tag_id(), tag_id::LONG);
        assert_eq!(NbtTag::Float(0.0).tag_id(), tag_id::FLOAT);
        assert_eq!(NbtTag::Double(0.0).tag_id(), tag_id::DOUBLE);
        assert_eq!(NbtTag::ByteArray(vec![]).tag_id(), tag_id::BYTE_ARRAY);
        assert_eq!(NbtTag::String("".into()).tag_id(), tag_id::STRING);
        assert_eq!(NbtTag::List(NbtList::new()).tag_id(), tag_id::LIST);
        assert_eq!(
            NbtTag::Compound(NbtCompound::new()).tag_id(),
            tag_id::COMPOUND
        );
        assert_eq!(NbtTag::IntArray(vec![]).tag_id(), tag_id::INT_ARRAY);
        assert_eq!(NbtTag::LongArray(vec![]).tag_id(), tag_id::LONG_ARRAY);
    }

    #[test]
    fn display() {
        assert_eq!(NbtTag::Byte(42).to_string(), "42b");
        assert_eq!(NbtTag::Short(100).to_string(), "100s");
        assert_eq!(NbtTag::Int(256).to_string(), "256");
        assert_eq!(NbtTag::Long(1000).to_string(), "1000L");
        assert_eq!(NbtTag::String("hello".into()).to_string(), "\"hello\"");
        assert_eq!(NbtTag::ByteArray(vec![1, 2, 3]).to_string(), "[B; 3 bytes]");
        assert_eq!(NbtTag::IntArray(vec![1, 2]).to_string(), "[I; 2 ints]");
        assert_eq!(NbtTag::LongArray(vec![1]).to_string(), "[L; 1 longs]");
    }

    // -- NbtList --

    #[test]
    fn list_new_is_empty() {
        let list = NbtList::new();
        assert!(list.is_empty());
        assert_eq!(list.len(), 0);
        assert_eq!(list.element_type, tag_id::END);
    }

    #[test]
    fn list_from_tags_homogeneous() {
        let tags = vec![NbtTag::Int(1), NbtTag::Int(2), NbtTag::Int(3)];
        let list = NbtList::from_tags(tags).unwrap();
        assert_eq!(list.element_type, tag_id::INT);
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn list_from_tags_empty() {
        let list = NbtList::from_tags(vec![]).unwrap();
        assert!(list.is_empty());
        assert_eq!(list.element_type, tag_id::END);
    }

    #[test]
    fn list_from_tags_mixed_returns_none() {
        let tags = vec![NbtTag::Int(1), NbtTag::String("hi".into())];
        assert!(NbtList::from_tags(tags).is_none());
    }

    // -- NbtCompound --

    #[test]
    fn compound_new_is_empty() {
        let compound = NbtCompound::new();
        assert!(compound.is_empty());
        assert_eq!(compound.len(), 0);
    }

    #[test]
    fn compound_insert_and_get() {
        let mut compound = NbtCompound::new();
        compound.insert("key", NbtTag::Int(42));
        assert_eq!(compound.get("key"), Some(&NbtTag::Int(42)));
        assert_eq!(compound.len(), 1);
    }

    #[test]
    fn compound_insert_replaces() {
        let mut compound = NbtCompound::new();
        compound.insert("key", NbtTag::Int(1));
        compound.insert("key", NbtTag::Int(2));
        assert_eq!(compound.get("key"), Some(&NbtTag::Int(2)));
        assert_eq!(compound.len(), 1);
    }

    #[test]
    fn compound_get_missing() {
        let compound = NbtCompound::new();
        assert!(compound.get("missing").is_none());
    }

    #[test]
    fn compound_contains_key() {
        let mut compound = NbtCompound::new();
        compound.insert("exists", NbtTag::Byte(1));
        assert!(compound.contains_key("exists"));
        assert!(!compound.contains_key("missing"));
    }

    #[test]
    fn compound_iter_preserves_order() {
        let mut compound = NbtCompound::new();
        compound.insert("first", NbtTag::Int(1));
        compound.insert("second", NbtTag::Int(2));
        compound.insert("third", NbtTag::Int(3));

        let names: Vec<&str> = compound.iter().map(|(n, _)| n).collect();
        assert_eq!(names, vec!["first", "second", "third"]);
    }
}
