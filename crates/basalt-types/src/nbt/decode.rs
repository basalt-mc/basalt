use crate::nbt::tag::{NbtCompound, NbtList, NbtTag, tag_id};
use crate::{Decode, Error, Result};

/// Reads an NBT string in the NBT wire format (u16-prefixed modified UTF-8).
///
/// NBT strings use a big-endian u16 length prefix, unlike protocol strings
/// which use VarInt. The maximum length is 65535 bytes (u16::MAX).
fn decode_nbt_string(buf: &mut &[u8]) -> Result<String> {
    let len = u16::decode(buf)? as usize;
    if buf.len() < len {
        return Err(Error::BufferUnderflow {
            needed: len,
            available: buf.len(),
        });
    }
    let (bytes, rest) = buf.split_at(len);
    let value = String::from_utf8(bytes.to_vec())?;
    *buf = rest;
    Ok(value)
}

/// Decodes the payload of an NBT tag given its type ID.
///
/// The type ID has already been read from the stream. This function reads
/// the remaining bytes for the tag's value based on its type. Used for
/// both compound entries (after reading type + name) and list elements
/// (after the list header declares the element type).
fn decode_tag_payload(tag_type: u8, buf: &mut &[u8]) -> Result<NbtTag> {
    match tag_type {
        tag_id::BYTE => Ok(NbtTag::Byte(i8::decode(buf)?)),
        tag_id::SHORT => Ok(NbtTag::Short(i16::decode(buf)?)),
        tag_id::INT => Ok(NbtTag::Int(i32::decode(buf)?)),
        tag_id::LONG => Ok(NbtTag::Long(i64::decode(buf)?)),
        tag_id::FLOAT => Ok(NbtTag::Float(f32::decode(buf)?)),
        tag_id::DOUBLE => Ok(NbtTag::Double(f64::decode(buf)?)),
        tag_id::BYTE_ARRAY => {
            let len = i32::decode(buf)?;
            if len < 0 {
                return Err(Error::Nbt(format!("negative byte array length: {len}")));
            }
            let len = len as usize;
            if buf.len() < len {
                return Err(Error::BufferUnderflow {
                    needed: len,
                    available: buf.len(),
                });
            }
            let mut data = Vec::with_capacity(len);
            for _ in 0..len {
                data.push(i8::decode(buf)?);
            }
            Ok(NbtTag::ByteArray(data))
        }
        tag_id::STRING => Ok(NbtTag::String(decode_nbt_string(buf)?)),
        tag_id::LIST => {
            let element_type = u8::decode(buf)?;
            let len = i32::decode(buf)?;
            if len < 0 {
                return Err(Error::Nbt(format!("negative list length: {len}")));
            }
            let len = len as usize;
            let mut elements = Vec::with_capacity(len);
            for _ in 0..len {
                elements.push(decode_tag_payload(element_type, buf)?);
            }
            Ok(NbtTag::List(NbtList {
                element_type,
                elements,
            }))
        }
        tag_id::COMPOUND => {
            let compound = decode_compound_payload(buf)?;
            Ok(NbtTag::Compound(compound))
        }
        tag_id::INT_ARRAY => {
            let len = i32::decode(buf)?;
            if len < 0 {
                return Err(Error::Nbt(format!("negative int array length: {len}")));
            }
            let len = len as usize;
            let mut data = Vec::with_capacity(len);
            for _ in 0..len {
                data.push(i32::decode(buf)?);
            }
            Ok(NbtTag::IntArray(data))
        }
        tag_id::LONG_ARRAY => {
            let len = i32::decode(buf)?;
            if len < 0 {
                return Err(Error::Nbt(format!("negative long array length: {len}")));
            }
            let len = len as usize;
            let mut data = Vec::with_capacity(len);
            for _ in 0..len {
                data.push(i64::decode(buf)?);
            }
            Ok(NbtTag::LongArray(data))
        }
        _ => Err(Error::Nbt(format!("unknown NBT tag type: {tag_type}"))),
    }
}

/// Decodes a compound payload: reads named entries until an End tag is found.
///
/// Each entry is: tag type byte + u16-prefixed name + payload.
/// The compound ends when a tag type of 0 (End) is encountered.
fn decode_compound_payload(buf: &mut &[u8]) -> Result<NbtCompound> {
    let mut compound = NbtCompound::new();
    loop {
        let tag_type = u8::decode(buf)?;
        if tag_type == tag_id::END {
            return Ok(compound);
        }
        let name = decode_nbt_string(buf)?;
        let tag = decode_tag_payload(tag_type, buf)?;
        compound.insert(name, tag);
    }
}

/// Decodes an NbtCompound from network NBT format.
///
/// Since Minecraft 1.20.3, network NBT uses a simplified root format:
/// a compound tag type byte (0x0A) followed directly by the compound
/// payload (no root tag name). This differs from the traditional NBT
/// format which includes a root tag name.
///
/// Fails if the root tag type is not Compound (0x0A).
impl Decode for NbtCompound {
    /// Reads the compound type byte, then decodes the compound payload.
    ///
    /// Fails with `Error::Nbt` if the root type is not Compound,
    /// or with other errors if the payload is malformed.
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        let tag_type = u8::decode(buf)?;
        if tag_type != tag_id::COMPOUND {
            return Err(Error::Nbt(format!(
                "expected compound root (type 10), got type {tag_type}"
            )));
        }
        decode_compound_payload(buf)
    }
}

/// Decodes an NbtTag from network NBT format.
///
/// The root is always expected to be a Compound tag. This delegates to
/// `NbtCompound::decode` and wraps the result in `NbtTag::Compound`.
impl Decode for NbtTag {
    /// Reads a network NBT root compound and wraps it as `NbtTag::Compound`.
    fn decode(buf: &mut &[u8]) -> Result<Self> {
        let compound = NbtCompound::decode(buf)?;
        Ok(NbtTag::Compound(compound))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Encode;

    /// Helper: encode then decode an NbtCompound and verify roundtrip.
    fn roundtrip_compound(compound: &NbtCompound) {
        let mut buf = Vec::new();
        compound.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let decoded = NbtCompound::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty(), "cursor not fully consumed");
        assert_eq!(decoded, *compound);
    }

    // -- Empty compound --

    #[test]
    fn empty_compound() {
        roundtrip_compound(&NbtCompound::new());
    }

    // -- Primitive types --

    #[test]
    fn compound_with_byte() {
        let mut c = NbtCompound::new();
        c.insert("value", NbtTag::Byte(42));
        roundtrip_compound(&c);
    }

    #[test]
    fn compound_with_short() {
        let mut c = NbtCompound::new();
        c.insert("value", NbtTag::Short(1234));
        roundtrip_compound(&c);
    }

    #[test]
    fn compound_with_int() {
        let mut c = NbtCompound::new();
        c.insert("value", NbtTag::Int(100000));
        roundtrip_compound(&c);
    }

    #[test]
    fn compound_with_long() {
        let mut c = NbtCompound::new();
        c.insert("value", NbtTag::Long(i64::MAX));
        roundtrip_compound(&c);
    }

    #[test]
    fn compound_with_float() {
        let mut c = NbtCompound::new();
        c.insert("value", NbtTag::Float(1.5));
        roundtrip_compound(&c);
    }

    #[test]
    fn compound_with_double() {
        let mut c = NbtCompound::new();
        c.insert("value", NbtTag::Double(1.23456789));
        roundtrip_compound(&c);
    }

    #[test]
    fn compound_with_string() {
        let mut c = NbtCompound::new();
        c.insert("name", NbtTag::String("hello world".into()));
        roundtrip_compound(&c);
    }

    // -- Array types --

    #[test]
    fn compound_with_byte_array() {
        let mut c = NbtCompound::new();
        c.insert("data", NbtTag::ByteArray(vec![1, 2, 3, -1, -128, 127]));
        roundtrip_compound(&c);
    }

    #[test]
    fn compound_with_int_array() {
        let mut c = NbtCompound::new();
        c.insert("heights", NbtTag::IntArray(vec![100, 200, -300]));
        roundtrip_compound(&c);
    }

    #[test]
    fn compound_with_long_array() {
        let mut c = NbtCompound::new();
        c.insert("states", NbtTag::LongArray(vec![i64::MIN, 0, i64::MAX]));
        roundtrip_compound(&c);
    }

    // -- List --

    #[test]
    fn compound_with_empty_list() {
        let mut c = NbtCompound::new();
        c.insert("items", NbtTag::List(NbtList::new()));
        roundtrip_compound(&c);
    }

    #[test]
    fn compound_with_int_list() {
        let mut c = NbtCompound::new();
        let list =
            NbtList::from_tags(vec![NbtTag::Int(1), NbtTag::Int(2), NbtTag::Int(3)]).unwrap();
        c.insert("scores", NbtTag::List(list));
        roundtrip_compound(&c);
    }

    #[test]
    fn compound_with_string_list() {
        let mut c = NbtCompound::new();
        let list = NbtList::from_tags(vec![
            NbtTag::String("alpha".into()),
            NbtTag::String("beta".into()),
        ])
        .unwrap();
        c.insert("names", NbtTag::List(list));
        roundtrip_compound(&c);
    }

    // -- Nested compounds --

    #[test]
    fn nested_compound() {
        let mut inner = NbtCompound::new();
        inner.insert("x", NbtTag::Int(10));
        inner.insert("y", NbtTag::Int(64));
        inner.insert("z", NbtTag::Int(-20));

        let mut outer = NbtCompound::new();
        outer.insert("pos", NbtTag::Compound(inner));
        outer.insert("name", NbtTag::String("marker".into()));
        roundtrip_compound(&outer);
    }

    #[test]
    fn deeply_nested() {
        let mut level3 = NbtCompound::new();
        level3.insert("deep", NbtTag::Byte(1));

        let mut level2 = NbtCompound::new();
        level2.insert("mid", NbtTag::Compound(level3));

        let mut level1 = NbtCompound::new();
        level1.insert("top", NbtTag::Compound(level2));
        roundtrip_compound(&level1);
    }

    // -- List of compounds --

    #[test]
    fn list_of_compounds() {
        let mut item1 = NbtCompound::new();
        item1.insert("id", NbtTag::String("minecraft:stone".into()));
        item1.insert("count", NbtTag::Byte(64));

        let mut item2 = NbtCompound::new();
        item2.insert("id", NbtTag::String("minecraft:dirt".into()));
        item2.insert("count", NbtTag::Byte(32));

        let list =
            NbtList::from_tags(vec![NbtTag::Compound(item1), NbtTag::Compound(item2)]).unwrap();

        let mut c = NbtCompound::new();
        c.insert("inventory", NbtTag::List(list));
        roundtrip_compound(&c);
    }

    // -- Multiple entries --

    #[test]
    fn compound_with_all_types() {
        let mut c = NbtCompound::new();
        c.insert("byte", NbtTag::Byte(i8::MAX));
        c.insert("short", NbtTag::Short(i16::MIN));
        c.insert("int", NbtTag::Int(42));
        c.insert("long", NbtTag::Long(i64::MAX));
        c.insert("float", NbtTag::Float(1.5));
        c.insert("double", NbtTag::Double(2.5));
        c.insert("string", NbtTag::String("test".into()));
        c.insert("byte_array", NbtTag::ByteArray(vec![1, 2]));
        c.insert("int_array", NbtTag::IntArray(vec![10, 20]));
        c.insert("long_array", NbtTag::LongArray(vec![100, 200]));
        c.insert("list", NbtTag::List(NbtList::new()));
        c.insert("compound", NbtTag::Compound(NbtCompound::new()));
        roundtrip_compound(&c);
    }

    // -- NbtTag root --

    #[test]
    fn nbt_tag_compound_roundtrip() {
        let mut c = NbtCompound::new();
        c.insert("value", NbtTag::Int(42));
        let tag = NbtTag::Compound(c.clone());

        let mut buf = Vec::new();
        tag.encode(&mut buf).unwrap();

        let mut cursor = buf.as_slice();
        let decoded = NbtTag::decode(&mut cursor).unwrap();
        assert!(cursor.is_empty());
        assert_eq!(decoded, NbtTag::Compound(c));
    }

    // -- Error cases --

    #[test]
    fn invalid_root_type() {
        // Root type byte is BYTE (1) instead of COMPOUND (10)
        let buf = [tag_id::BYTE, 42];
        let mut cursor = buf.as_slice();
        assert!(matches!(
            NbtCompound::decode(&mut cursor),
            Err(Error::Nbt(_))
        ));
    }

    #[test]
    fn empty_buffer() {
        let mut cursor: &[u8] = &[];
        assert!(NbtCompound::decode(&mut cursor).is_err());
    }

    #[test]
    fn truncated_compound() {
        // Compound type byte but no payload
        let buf = [tag_id::COMPOUND];
        let mut cursor = buf.as_slice();
        assert!(NbtCompound::decode(&mut cursor).is_err());
    }
}
