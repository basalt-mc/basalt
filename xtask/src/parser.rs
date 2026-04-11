//! Protocol JSON parser for packet definitions.
//!
//! Reads the minecraft-data `protocol.json` structure and converts it
//! into `PacketDef` / `FieldDef` / `InlineStruct` values that the code
//! generator can turn into Rust source code.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::helpers::{to_pascal_case, to_snake_case};
use crate::types::{FieldDef, InlineStruct, PacketDef};

/// Parses all packets for one direction (serverbound or clientbound)
/// within a protocol state.
///
/// Reads the packet mapper to discover packet names and IDs, then
/// parses each packet's container fields into a `PacketDef`. Packets
/// that redirect to `packet_common_*` types are skipped — those are
/// shared across states and handled separately.
pub(crate) fn parse_direction(
    state: &Value,
    direction: &str,
    dir_prefix: &str,
    state_pascal: &str,
) -> Vec<PacketDef> {
    let dir_data = &state[direction];
    if dir_data.is_null() {
        return Vec::new();
    }

    let types = &dir_data["types"];
    let packet_mapper = &types["packet"];
    if packet_mapper.is_null() {
        return Vec::new();
    }

    let mappings = &packet_mapper[1][0]["type"][1]["mappings"];
    let id_map: BTreeMap<String, String> = mappings
        .as_object()
        .expect("mappings should be an object")
        .iter()
        .map(|(id, name)| (name.as_str().unwrap().to_string(), id.clone()))
        .collect();

    let mut packets = Vec::new();
    for (name, id) in &id_map {
        let type_key = format!("packet_{name}");

        // Check if the switch redirects to a common packet
        let switch_fields = &packet_mapper[1][1]["type"][1]["fields"];
        let actual_type_key = switch_fields[name.as_str()].as_str().unwrap_or(&type_key);

        if actual_type_key.starts_with("packet_common_") {
            continue;
        }

        let packet_type = &types[actual_type_key];
        if packet_type.is_null() {
            eprintln!("Warning: packet type {actual_type_key} not found, skipping");
            continue;
        }

        let struct_name = format!("{dir_prefix}{state_pascal}{}", to_pascal_case(name));
        let (fields, inline_structs) = parse_container_fields(packet_type, &struct_name, types);

        packets.push(PacketDef {
            name: struct_name,
            id: id.clone(),
            fields,
            inline_structs,
        });
    }

    packets
}

/// Parses the fields of a `["container", [...]]` type definition.
///
/// Each field in the container array is mapped to a Rust type via
/// `map_type`. Container fields that contain embedded container types
/// (e.g., arrays of structs) produce `InlineStruct` entries that must
/// be emitted as separate structs before the parent.
pub(crate) fn parse_container_fields(
    container: &Value,
    parent_name: &str,
    types_ctx: &Value,
) -> (Vec<FieldDef>, Vec<InlineStruct>) {
    let mut fields = Vec::new();
    let mut inline_structs = Vec::new();

    let field_array = container[1]
        .as_array()
        .expect("container fields should be an array");

    for (i, field) in field_array.iter().enumerate() {
        let field_name = field["name"]
            .as_str()
            .expect("field should have a name")
            .to_string();
        let rust_name = to_snake_case(&field_name);
        let field_type = &field["type"];
        let is_last = i == field_array.len() - 1;

        let (rust_type, attribute, inline) =
            map_type(field_type, parent_name, &field_name, is_last, types_ctx);

        if let Some(inline_struct) = inline {
            inline_structs.push(inline_struct);
        }

        fields.push(FieldDef {
            name: rust_name,
            rust_type,
            attribute,
        });
    }

    (fields, inline_structs)
}

/// Maps a protocol JSON type definition to a Rust type.
///
/// Handles primitive types (varint, string, bool, numeric), compound
/// types (buffer, option, array, mapper, bitflags), and custom types
/// defined in the state's type context (e.g., SpawnInfo containers).
///
/// Returns a tuple of (rust_type, optional_field_attribute, optional_inline_struct).
pub(crate) fn map_type(
    type_def: &Value,
    parent_name: &str,
    field_name: &str,
    is_last: bool,
    types_ctx: &Value,
) -> (String, Option<String>, Option<InlineStruct>) {
    match type_def {
        Value::String(s) => match s.as_str() {
            "varint" => ("i32".into(), Some("varint".into()), None),
            "varlong" => ("i64".into(), Some("varlong".into()), None),
            "string" => ("String".into(), None, None),
            "bool" => ("bool".into(), None, None),
            "u8" => ("u8".into(), None, None),
            "u16" => ("u16".into(), None, None),
            "u32" => ("u32".into(), None, None),
            "u64" => ("u64".into(), None, None),
            "i8" => ("i8".into(), None, None),
            "i16" => ("i16".into(), None, None),
            "i32" => ("i32".into(), None, None),
            "i64" => ("i64".into(), None, None),
            "f32" => ("f32".into(), None, None),
            "f64" => ("f64".into(), None, None),
            "UUID" => ("Uuid".into(), None, None),
            "position" => ("Position".into(), None, None),
            "anonymousNbt" => ("NbtCompound".into(), None, None),
            "anonOptionalNbt" => ("Option<NbtCompound>".into(), Some("optional".into()), None),
            "ByteArray" => ("Vec<u8>".into(), Some("length = \"varint\"".into()), None),
            "ContainerID" | "optvarint" => ("i32".into(), Some("varint".into()), None),
            "soundSource" => ("i32".into(), Some("varint".into()), None),
            "Slot" => ("Slot".into(), None, None),
            "vec2f" => ("Vec2f".into(), None, None),
            "vec3f" => ("Vec3f".into(), None, None),
            "vec3f64" => ("Vec3f64".into(), None, None),
            "vec3i16" => ("Vec3i16".into(), None, None),
            "packedChunkPos" => ("i64".into(), None, None),
            "restBuffer" => {
                if is_last {
                    ("Vec<u8>".into(), Some("rest".into()), None)
                } else {
                    ("Vec<u8>".into(), None, None)
                }
            }
            other => {
                // Try to resolve the type from the state's type definitions.
                // Custom types like SpawnInfo are defined as containers in the
                // direction's "types" section and referenced by name in packet
                // field definitions.
                let resolved = &types_ctx[other];
                if !resolved.is_null() {
                    if resolved.is_array() && resolved[0].as_str() == Some("container") {
                        // The type is an inline container — generate a nested
                        // struct so its fields are encoded/decoded inline
                        // (no length prefix), matching the wire format.
                        let struct_name = format!("{}{}", parent_name, to_pascal_case(other));
                        let (inner_fields, _nested) =
                            parse_container_fields(resolved, &struct_name, types_ctx);
                        let inline = InlineStruct {
                            name: struct_name.clone(),
                            fields: inner_fields,
                        };
                        (struct_name, None, Some(inline))
                    } else {
                        // The type is an alias — resolve it recursively.
                        map_type(resolved, parent_name, field_name, is_last, types_ctx)
                    }
                } else {
                    eprintln!("Warning: unknown type '{other}', using Vec<u8>");
                    ("Vec<u8>".into(), None, None)
                }
            }
        },
        Value::Array(arr) if arr.len() == 2 => {
            let type_name = arr[0].as_str().unwrap_or("");
            match type_name {
                "buffer" => {
                    let count_type = arr[1]["countType"].as_str().unwrap_or("varint");
                    if count_type == "varint" {
                        ("Vec<u8>".into(), Some("length = \"varint\"".into()), None)
                    } else {
                        ("Vec<u8>".into(), None, None)
                    }
                }
                "option" => {
                    let inner_type = &arr[1];
                    let (inner_rust, _inner_attr, inline) =
                        map_type(inner_type, parent_name, field_name, is_last, types_ctx);
                    // If the inner type produced an inline struct, we can't
                    // wrap it in Option because Vec<InlineStruct> doesn't
                    // implement Encode/Decode. Fall back to opaque bytes.
                    if inline.is_some() {
                        ("Option<Vec<u8>>".into(), Some("optional".into()), None)
                    } else {
                        (
                            format!("Option<{inner_rust}>"),
                            Some("optional".into()),
                            None,
                        )
                    }
                }
                "array" => {
                    let count_type = arr[1]["countType"].as_str().unwrap_or("varint");
                    let inner_type = &arr[1]["type"];

                    if inner_type.is_array() && inner_type[0].as_str() == Some("container") {
                        let struct_name = format!("{}{}", parent_name, to_pascal_case(field_name));
                        let (inner_fields, nested_inlines) =
                            parse_container_fields(inner_type, &struct_name, types_ctx);

                        // For nested inline structs (arrays inside arrays),
                        // we need to handle them as Vec<u8> because our derive
                        // system doesn't support nested generic containers.
                        // The top-level inline struct fields that reference
                        // nested inlines will use Vec<u8> fallback.
                        let clean_fields = if nested_inlines.is_empty() {
                            inner_fields
                        } else {
                            // Replace fields that reference nested inlines
                            // with Vec<u8> — we can't derive Encode/Decode
                            // for Vec<NestedInlineStruct>
                            inner_fields
                                .into_iter()
                                .map(|f| {
                                    if nested_inlines
                                        .iter()
                                        .any(|ni| f.rust_type.contains(&ni.name))
                                    {
                                        FieldDef {
                                            name: f.name,
                                            rust_type: "Vec<u8>".into(),
                                            attribute: Some("length = \"varint\"".into()),
                                        }
                                    } else {
                                        f
                                    }
                                })
                                .collect()
                        };

                        let inline = InlineStruct {
                            name: struct_name.clone(),
                            fields: clean_fields,
                        };

                        if count_type == "varint" {
                            (
                                format!("Vec<{struct_name}>"),
                                Some("length = \"varint\"".into()),
                                Some(inline),
                            )
                        } else {
                            (format!("Vec<{struct_name}>"), None, Some(inline))
                        }
                    } else {
                        let (inner_rust, _, inline) =
                            map_type(inner_type, parent_name, field_name, false, types_ctx);
                        if count_type == "varint" {
                            (
                                format!("Vec<{inner_rust}>"),
                                Some("length = \"varint\"".into()),
                                inline,
                            )
                        } else {
                            (format!("Vec<{inner_rust}>"), None, inline)
                        }
                    }
                }
                "mapper" | "bitflags" => {
                    // A mapper wraps a numeric type with named constants,
                    // and a bitflags wraps an integer with named bit positions.
                    // We ignore the mappings/flags and use the underlying type
                    // directly (e.g., ["mapper", {"type": "u8", ...}] → u8,
                    // ["bitflags", {"type": "u32", ...}] → u32).
                    let inner_type = &arr[1]["type"];
                    map_type(inner_type, parent_name, field_name, is_last, types_ctx)
                }
                _ => {
                    eprintln!("Warning: unknown compound type '{type_name}', using Vec<u8>");
                    ("Vec<u8>".into(), None, None)
                }
            }
        }
        _ => {
            eprintln!("Warning: unexpected type format, using Vec<u8>");
            ("Vec<u8>".into(), None, None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- map_type --

    #[test]
    fn map_varint() {
        let (ty, attr, _) = map_type(&Value::String("varint".into()), "", "", false, &Value::Null);
        assert_eq!(ty, "i32");
        assert_eq!(attr, Some("varint".into()));
    }

    #[test]
    fn map_string() {
        let (ty, attr, _) = map_type(&Value::String("string".into()), "", "", false, &Value::Null);
        assert_eq!(ty, "String");
        assert!(attr.is_none());
    }

    #[test]
    fn map_uuid() {
        let (ty, attr, _) = map_type(&Value::String("UUID".into()), "", "", false, &Value::Null);
        assert_eq!(ty, "Uuid");
        assert!(attr.is_none());
    }

    #[test]
    fn map_bool() {
        let (ty, attr, _) = map_type(&Value::String("bool".into()), "", "", false, &Value::Null);
        assert_eq!(ty, "bool");
        assert!(attr.is_none());
    }

    #[test]
    fn map_rest_buffer_last() {
        let (ty, attr, _) = map_type(
            &Value::String("restBuffer".into()),
            "",
            "",
            true,
            &Value::Null,
        );
        assert_eq!(ty, "Vec<u8>");
        assert_eq!(attr, Some("rest".into()));
    }

    #[test]
    fn map_rest_buffer_not_last() {
        let (ty, attr, _) = map_type(
            &Value::String("restBuffer".into()),
            "",
            "",
            false,
            &Value::Null,
        );
        assert_eq!(ty, "Vec<u8>");
        assert!(attr.is_none());
    }

    #[test]
    fn map_buffer_varint() {
        let json: Value = serde_json::from_str(r#"["buffer", {"countType": "varint"}]"#).unwrap();
        let (ty, attr, _) = map_type(&json, "", "", false, &Value::Null);
        assert_eq!(ty, "Vec<u8>");
        assert_eq!(attr, Some("length = \"varint\"".into()));
    }

    #[test]
    fn map_option_string() {
        let json: Value = serde_json::from_str(r#"["option", "string"]"#).unwrap();
        let (ty, attr, _) = map_type(&json, "", "", false, &Value::Null);
        assert_eq!(ty, "Option<String>");
        assert_eq!(attr, Some("optional".into()));
    }

    #[test]
    fn map_numeric_types() {
        for (json_type, rust_type) in [
            ("u8", "u8"),
            ("u16", "u16"),
            ("i8", "i8"),
            ("i16", "i16"),
            ("i32", "i32"),
            ("i64", "i64"),
            ("f32", "f32"),
            ("f64", "f64"),
        ] {
            let (ty, attr, _) = map_type(
                &Value::String(json_type.into()),
                "",
                "",
                false,
                &Value::Null,
            );
            assert_eq!(ty, rust_type, "failed for {json_type}");
            assert!(attr.is_none(), "unexpected attr for {json_type}");
        }
    }

    #[test]
    fn map_array_of_primitives() {
        let json: Value =
            serde_json::from_str(r#"["array", {"countType": "varint", "type": "i32"}]"#).unwrap();
        let (ty, attr, _) = map_type(&json, "", "", false, &Value::Null);
        assert_eq!(ty, "Vec<i32>");
        assert_eq!(attr, Some("length = \"varint\"".into()));
    }

    #[test]
    fn map_varlong() {
        let (ty, attr, _) = map_type(
            &Value::String("varlong".into()),
            "",
            "",
            false,
            &Value::Null,
        );
        assert_eq!(ty, "i64");
        assert_eq!(attr, Some("varlong".into()));
    }

    #[test]
    fn map_unknown_type_falls_back() {
        let (ty, _, _) = map_type(
            &Value::String("unknownType".into()),
            "",
            "",
            false,
            &Value::Null,
        );
        assert_eq!(ty, "Vec<u8>");
    }

    #[test]
    fn map_position() {
        let (ty, attr, _) = map_type(
            &Value::String("position".into()),
            "",
            "",
            false,
            &Value::Null,
        );
        assert_eq!(ty, "Position");
        assert!(attr.is_none());
    }

    #[test]
    fn map_anonymous_nbt() {
        let (ty, attr, _) = map_type(
            &Value::String("anonymousNbt".into()),
            "",
            "",
            false,
            &Value::Null,
        );
        assert_eq!(ty, "NbtCompound");
        assert!(attr.is_none());
    }

    #[test]
    fn map_anon_optional_nbt() {
        let (ty, attr, _) = map_type(
            &Value::String("anonOptionalNbt".into()),
            "",
            "",
            false,
            &Value::Null,
        );
        assert_eq!(ty, "Option<NbtCompound>");
        assert_eq!(attr, Some("optional".into()));
    }

    #[test]
    fn map_byte_array_type() {
        let (ty, attr, _) = map_type(
            &Value::String("ByteArray".into()),
            "",
            "",
            false,
            &Value::Null,
        );
        assert_eq!(ty, "Vec<u8>");
        assert_eq!(attr, Some("length = \"varint\"".into()));
    }

    #[test]
    fn map_container_id() {
        let (ty, attr, _) = map_type(
            &Value::String("ContainerID".into()),
            "",
            "",
            false,
            &Value::Null,
        );
        assert_eq!(ty, "i32");
        assert_eq!(attr, Some("varint".into()));
    }

    #[test]
    fn map_slot() {
        let (ty, attr, _) = map_type(&Value::String("Slot".into()), "", "", false, &Value::Null);
        assert_eq!(ty, "Slot");
        assert!(attr.is_none());
    }

    #[test]
    fn map_vec2f() {
        let (ty, _, _) = map_type(&Value::String("vec2f".into()), "", "", false, &Value::Null);
        assert_eq!(ty, "Vec2f");
    }

    #[test]
    fn map_vec3f() {
        let (ty, _, _) = map_type(&Value::String("vec3f".into()), "", "", false, &Value::Null);
        assert_eq!(ty, "Vec3f");
    }

    #[test]
    fn map_vec3f64() {
        let (ty, _, _) = map_type(
            &Value::String("vec3f64".into()),
            "",
            "",
            false,
            &Value::Null,
        );
        assert_eq!(ty, "Vec3f64");
    }

    #[test]
    fn map_vec3i16() {
        let (ty, _, _) = map_type(
            &Value::String("vec3i16".into()),
            "",
            "",
            false,
            &Value::Null,
        );
        assert_eq!(ty, "Vec3i16");
    }

    #[test]
    fn map_sound_source() {
        let (ty, attr, _) = map_type(
            &Value::String("soundSource".into()),
            "",
            "",
            false,
            &Value::Null,
        );
        assert_eq!(ty, "i32");
        assert_eq!(attr, Some("varint".into()));
    }

    #[test]
    fn map_packed_chunk_pos() {
        let (ty, _, _) = map_type(
            &Value::String("packedChunkPos".into()),
            "",
            "",
            false,
            &Value::Null,
        );
        assert_eq!(ty, "i64");
    }

    #[test]
    fn map_optvarint() {
        let (ty, attr, _) = map_type(
            &Value::String("optvarint".into()),
            "",
            "",
            false,
            &Value::Null,
        );
        assert_eq!(ty, "i32");
        assert_eq!(attr, Some("varint".into()));
    }

    #[test]
    fn map_option_with_inline_falls_back() {
        let json: Value = serde_json::from_str(
            r#"["option", ["array", {
            "countType": "varint",
            "type": ["container", [{"name": "x", "type": "i32"}]]
        }]]"#,
        )
        .unwrap();
        let (ty, attr, inline) = map_type(&json, "Test", "field", false, &Value::Null);
        assert_eq!(ty, "Option<Vec<u8>>");
        assert_eq!(attr, Some("optional".into()));
        assert!(inline.is_none());
    }

    // -- parse_container_fields --

    #[test]
    fn parse_simple_container() {
        let json: Value = serde_json::from_str(
            r#"
            ["container", [
                {"name": "username", "type": "string"},
                {"name": "age", "type": "varint"}
            ]]
        "#,
        )
        .unwrap();

        let (fields, inlines) = parse_container_fields(&json, "TestPacket", &Value::Null);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "username");
        assert_eq!(fields[0].rust_type, "String");
        assert!(fields[0].attribute.is_none());
        assert_eq!(fields[1].name, "age");
        assert_eq!(fields[1].rust_type, "i32");
        assert_eq!(fields[1].attribute, Some("varint".into()));
        assert!(inlines.is_empty());
    }

    #[test]
    fn parse_container_with_rest_buffer() {
        let json: Value = serde_json::from_str(
            r#"
            ["container", [
                {"name": "id", "type": "varint"},
                {"name": "data", "type": "restBuffer"}
            ]]
        "#,
        )
        .unwrap();

        let (fields, _) = parse_container_fields(&json, "TestPacket", &Value::Null);
        assert_eq!(fields[1].name, "data");
        assert_eq!(fields[1].rust_type, "Vec<u8>");
        assert_eq!(fields[1].attribute, Some("rest".into()));
    }

    #[test]
    fn parse_container_with_inline_struct() {
        let json: Value = serde_json::from_str(
            r#"
            ["container", [
                {"name": "items", "type": ["array", {
                    "countType": "varint",
                    "type": ["container", [
                        {"name": "name", "type": "string"},
                        {"name": "value", "type": "i32"}
                    ]]
                }]}
            ]]
        "#,
        )
        .unwrap();

        let (fields, inlines) = parse_container_fields(&json, "TestPacket", &Value::Null);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].rust_type, "Vec<TestPacketItems>");
        assert_eq!(fields[0].attribute, Some("length = \"varint\"".into()));
        assert_eq!(inlines.len(), 1);
        assert_eq!(inlines[0].name, "TestPacketItems");
        assert_eq!(inlines[0].fields.len(), 2);
    }

    #[test]
    fn parse_empty_container() {
        let json: Value = serde_json::from_str(r#"["container", []]"#).unwrap();
        let (fields, inlines) = parse_container_fields(&json, "TestPacket", &Value::Null);
        assert!(fields.is_empty());
        assert!(inlines.is_empty());
    }

    // -- parse_direction --

    #[test]
    fn parse_direction_skips_common_packets() {
        let json: Value = serde_json::from_str(r#"{
            "toServer": {
                "types": {
                    "packet": ["container", [
                        {"name": "name", "type": ["mapper", {"type": "varint", "mappings": {"0x00": "login_start", "0x01": "cookie_response"}}]},
                        {"name": "params", "type": ["switch", {"compareTo": "name", "fields": {"login_start": "packet_login_start", "cookie_response": "packet_common_cookie_response"}}]}
                    ]],
                    "packet_login_start": ["container", [{"name": "username", "type": "string"}]]
                }
            }
        }"#).unwrap();

        let packets = parse_direction(&json, "toServer", "Serverbound", "Login");
        // Should only have login_start, cookie_response should be skipped
        assert_eq!(packets.len(), 1);
        assert!(packets[0].name.contains("LoginStart"));
    }

    #[test]
    fn parse_direction_empty() {
        let json: Value = serde_json::from_str(r#"{}"#).unwrap();
        let packets = parse_direction(&json, "toServer", "Serverbound", "Login");
        assert!(packets.is_empty());
    }
}
