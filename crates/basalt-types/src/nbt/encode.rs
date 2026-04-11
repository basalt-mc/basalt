use crate::nbt::tag::{NbtCompound, NbtTag, tag_id};
use crate::{Encode, EncodedSize, Result};

/// Writes an NBT string in the NBT wire format (u16-prefixed modified UTF-8).
///
/// NBT strings use a big-endian u16 length prefix, not VarInt like protocol
/// strings. This is a deliberate difference between the NBT format and the
/// outer protocol string encoding.
fn encode_nbt_string(s: &str, buf: &mut Vec<u8>) -> Result<()> {
    let bytes = s.as_bytes();
    (bytes.len() as u16).encode(buf)?;
    buf.extend_from_slice(bytes);
    Ok(())
}

/// Computes the wire size of an NBT string (u16 prefix + UTF-8 bytes).
fn nbt_string_size(s: &str) -> usize {
    2 + s.len()
}

/// Encodes only the payload of an NBT tag (without the type byte or name).
///
/// This is used internally for list elements (which share a single type
/// byte for the whole list) and for compound entry values (where the type
/// byte and name are written separately).
fn encode_tag_payload(tag: &NbtTag, buf: &mut Vec<u8>) -> Result<()> {
    match tag {
        NbtTag::Byte(v) => v.encode(buf),
        NbtTag::Short(v) => v.encode(buf),
        NbtTag::Int(v) => v.encode(buf),
        NbtTag::Long(v) => v.encode(buf),
        NbtTag::Float(v) => v.encode(buf),
        NbtTag::Double(v) => v.encode(buf),
        NbtTag::ByteArray(v) => {
            (v.len() as i32).encode(buf)?;
            for &b in v {
                b.encode(buf)?;
            }
            Ok(())
        }
        NbtTag::String(v) => encode_nbt_string(v, buf),
        NbtTag::List(list) => {
            list.element_type.encode(buf)?;
            (list.elements.len() as i32).encode(buf)?;
            for elem in &list.elements {
                encode_tag_payload(elem, buf)?;
            }
            Ok(())
        }
        NbtTag::Compound(compound) => {
            encode_compound_payload(compound, buf)?;
            Ok(())
        }
        NbtTag::IntArray(v) => {
            (v.len() as i32).encode(buf)?;
            for &val in v {
                val.encode(buf)?;
            }
            Ok(())
        }
        NbtTag::LongArray(v) => {
            (v.len() as i32).encode(buf)?;
            for &val in v {
                val.encode(buf)?;
            }
            Ok(())
        }
    }
}

/// Encodes the payload of a compound tag: each named entry followed by an End tag.
///
/// Each entry is: tag type byte + u16-prefixed name + payload.
/// The compound is terminated by a single End tag (0x00).
fn encode_compound_payload(compound: &NbtCompound, buf: &mut Vec<u8>) -> Result<()> {
    for (name, tag) in compound.iter() {
        tag.tag_id().encode(buf)?;
        encode_nbt_string(name, buf)?;
        encode_tag_payload(tag, buf)?;
    }
    tag_id::END.encode(buf)?;
    Ok(())
}

/// Computes the wire size of an NBT tag payload (without type byte or name).
fn tag_payload_size(tag: &NbtTag) -> usize {
    match tag {
        NbtTag::Byte(_) => 1,
        NbtTag::Short(_) => 2,
        NbtTag::Int(_) => 4,
        NbtTag::Long(_) => 8,
        NbtTag::Float(_) => 4,
        NbtTag::Double(_) => 8,
        NbtTag::ByteArray(v) => 4 + v.len(),
        NbtTag::String(v) => nbt_string_size(v),
        NbtTag::List(list) => {
            // element type (1) + count (4) + payloads
            1 + 4 + list.elements.iter().map(tag_payload_size).sum::<usize>()
        }
        NbtTag::Compound(compound) => compound_payload_size(compound),
        NbtTag::IntArray(v) => 4 + v.len() * 4,
        NbtTag::LongArray(v) => 4 + v.len() * 8,
    }
}

/// Computes the wire size of a compound payload (entries + End tag).
fn compound_payload_size(compound: &NbtCompound) -> usize {
    let entries_size: usize = compound
        .iter()
        .map(|(name, tag)| {
            // type byte (1) + name (u16 prefix + bytes) + payload
            1 + nbt_string_size(name) + tag_payload_size(tag)
        })
        .sum();
    entries_size + 1 // +1 for the End tag
}

/// Encodes an NbtCompound as a network NBT root compound.
///
/// Since Minecraft 1.20.3, network NBT uses a simplified root format:
/// a compound tag type byte (0x0A) followed directly by the compound
/// payload (no root tag name). This differs from the traditional NBT
/// format which includes a root tag name after the type byte.
///
/// This encoder produces the network NBT format used in modern protocol
/// packets (chat components, item stacks, registry data).
impl Encode for NbtCompound {
    /// Writes the compound type byte (0x0A) followed by the compound payload.
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        tag_id::COMPOUND.encode(buf)?;
        encode_compound_payload(self, buf)
    }
}

/// Computes the wire size of a network NBT root compound.
///
/// Includes the compound type byte (1) plus the compound payload size
/// (entries + End tag).
impl EncodedSize for NbtCompound {
    /// Returns 1 (type byte) + payload size.
    fn encoded_size(&self) -> usize {
        1 + compound_payload_size(self)
    }
}

/// Encodes a single NbtTag as a network NBT root.
///
/// Only `NbtTag::Compound` is valid as a network NBT root. This delegates
/// to the `NbtCompound` encoder for compound tags. Other tag types are
/// wrapped in a single-entry compound for compatibility, though in practice
/// the protocol always uses compound roots.
impl Encode for NbtTag {
    /// Writes the tag as a network NBT root compound.
    fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        match self {
            NbtTag::Compound(compound) => compound.encode(buf),
            _ => {
                // Wrap non-compound tags in a compound with an empty-string key
                let mut wrapper = NbtCompound::new();
                wrapper.insert("", self.clone());
                wrapper.encode(buf)
            }
        }
    }
}

/// Computes the wire size of an NbtTag as a network NBT root.
impl EncodedSize for NbtTag {
    fn encoded_size(&self) -> usize {
        match self {
            NbtTag::Compound(compound) => compound.encoded_size(),
            _ => {
                let mut wrapper = NbtCompound::new();
                wrapper.insert("", self.clone());
                wrapper.encoded_size()
            }
        }
    }
}
