//! Hand-rolled `IDSet` — the one protocol type kept manual.
//!
//! `IDSet` (per minecraft-data 1.21.4: `registryEntryHolderSet`)
//! encodes the variant count in the discriminator tag itself
//! (`tag = N + 1` means N inline varints follow). No other Mojang
//! type uses this "tag-is-a-count" pattern, so generalising it in
//! the codegen IR would be more code than it saves. It lives here
//! as a single, focused exception.
//!
//! The codegen leaves `crafting_requirements: Option<Vec<u8>>` as
//! `Option<Vec<u8>>` and the server always sends `None` — the wire
//! byte is `false`, identical to what `Option<Vec<IDSet>> = None`
//! would produce. Plugins that later need typed IDSet predicates
//! can add combined-attribute support to `basalt-derive` and switch
//! the codegen field over.

use basalt_types::{Encode, EncodedSize, Result, VarInt};

/// A set of registry entries — referenced either by name (an inline
/// tag identifier) or by an inline list of registry ids.
///
/// Wire format (`registryEntryHolderSet` per minecraft-data 1.21.4):
///
/// ```text
/// IDSet := varint tag
///   if tag == 0: name: string
///   else:        ids: varint[tag - 1]
/// ```
///
/// Used by `crafting_requirements` on a recipe-book entry to gate
/// when the recipe should appear in the book (e.g. "only show if the
/// player carries an item from this tag").
#[derive(Debug, Clone, PartialEq)]
pub enum IDSet {
    /// Reference a registered tag by its identifier
    /// (e.g. `"minecraft:logs"`).
    Tag(String),
    /// Inline list of registry ids.
    Ids(Vec<i32>),
}

impl Encode for IDSet {
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Tag(name) => {
                VarInt(0).encode(buf)?;
                name.encode(buf)
            }
            Self::Ids(ids) => {
                // Tag = ids.len() + 1; ids are written without an
                // additional length prefix (the length is recovered
                // from the tag by the reader).
                VarInt((ids.len() as i32) + 1).encode(buf)?;
                for id in ids {
                    VarInt(*id).encode(buf)?;
                }
                Ok(())
            }
        }
    }
}

impl EncodedSize for IDSet {
    fn encoded_size(&self) -> usize {
        match self {
            Self::Tag(name) => VarInt(0).encoded_size() + name.encoded_size(),
            Self::Ids(ids) => {
                VarInt((ids.len() as i32) + 1).encoded_size()
                    + ids
                        .iter()
                        .map(|id| VarInt(*id).encoded_size())
                        .sum::<usize>()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip<T: Encode + EncodedSize>(value: &T) -> Vec<u8> {
        let mut buf = Vec::new();
        value.encode(&mut buf).expect("encode");
        assert_eq!(
            buf.len(),
            value.encoded_size(),
            "encoded_size disagrees with encode output length"
        );
        buf
    }

    #[test]
    fn id_set_tag_round_trip() {
        let set = IDSet::Tag("minecraft:planks".into());
        let bytes = roundtrip(&set);
        // tag 0 | varint length 16 | "minecraft:planks"
        let mut expected = vec![0x00, 16];
        expected.extend_from_slice(b"minecraft:planks");
        assert_eq!(bytes, expected);
    }

    #[test]
    fn id_set_ids_writes_offset_tag() {
        let set = IDSet::Ids(vec![1, 2, 3]);
        let bytes = roundtrip(&set);
        // tag = 4 (3 ids + 1) | 1 | 2 | 3
        assert_eq!(bytes, vec![0x04, 0x01, 0x02, 0x03]);
    }

    #[test]
    fn id_set_empty_ids_uses_tag_one() {
        let set = IDSet::Ids(vec![]);
        let bytes = roundtrip(&set);
        // tag = 1 (0 ids + 1)
        assert_eq!(bytes, vec![0x01]);
    }
}
