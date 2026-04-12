//! Type registry for resolving protocol JSON into the IR.
//!
//! `TypeRegistry` encapsulates the merged local + global type
//! definitions and provides methods to resolve JSON type references
//! into `ProtocolType` values. This replaces the old `map_type`
//! function and `types_ctx: &Value` threading.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::helpers::{to_pascal_case, to_snake_case};
use crate::types::{CountType, PacketDef, ProtocolType, ResolvedField, SwitchVariant};

/// A registry of type definitions used to resolve protocol JSON
/// into the `ProtocolType` IR.
///
/// Created once per direction (serverbound/clientbound) by merging
/// the direction's local types with the global protocol types.
/// Local definitions take priority over global ones.
pub(crate) struct TypeRegistry {
    types: Map<String, Value>,
}

impl TypeRegistry {
    /// Creates a new registry by merging local and global types.
    pub fn new(local: &Value, global: &Value) -> Self {
        let mut types = Map::new();
        if let Some(global_obj) = global.as_object() {
            for (k, v) in global_obj {
                types.insert(k.clone(), v.clone());
            }
        }
        if let Some(local_obj) = local.as_object() {
            for (k, v) in local_obj {
                types.insert(k.clone(), v.clone());
            }
        }
        Self { types }
    }

    /// Parses all packets for one direction within a protocol state.
    pub fn parse_direction(
        &self,
        state: &Value,
        direction: &str,
        dir_prefix: &str,
        state_pascal: &str,
    ) -> Vec<PacketDef> {
        let dir_data = &state[direction];
        if dir_data.is_null() {
            return Vec::new();
        }

        let local_types = &dir_data["types"];
        let packet_mapper = &local_types["packet"];
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

        let merged = Value::Object(self.types.clone());
        let mut packets = Vec::new();

        for (name, id) in &id_map {
            let type_key = format!("packet_{name}");
            let switch_fields = &packet_mapper[1][1]["type"][1]["fields"];
            let actual_type_key = switch_fields[name.as_str()].as_str().unwrap_or(&type_key);

            if actual_type_key.starts_with("packet_common_") {
                continue;
            }

            let packet_type = &merged[actual_type_key];
            let is_container = packet_type.is_array()
                && packet_type
                    .as_array()
                    .is_some_and(|a| a[0].as_str() == Some("container"));
            if packet_type.is_null() || !is_container {
                continue;
            }

            let struct_name = format!("{dir_prefix}{state_pascal}{}", to_pascal_case(name));
            let fields = self.parse_container(packet_type, &struct_name, true);

            packets.push(PacketDef {
                name: struct_name,
                id: id.clone(),
                fields,
            });
        }

        packets
    }

    /// Parses a `["container", [...]]` into a list of resolved fields.
    ///
    /// When `resolve_switches` is true (top-level packets only), switch
    /// field groups are detected and replaced with `SwitchEnum` types.
    fn parse_container(
        &self,
        container: &Value,
        parent_name: &str,
        resolve_switches: bool,
    ) -> Vec<ResolvedField> {
        let field_array = container[1]
            .as_array()
            .expect("container fields should be an array");

        let switch_group = if resolve_switches {
            self.detect_switch_group(field_array, parent_name)
        } else {
            None
        };

        let mut fields = Vec::new();

        for (i, field) in field_array.iter().enumerate() {
            let field_name = match field["name"].as_str() {
                Some(name) => name.to_string(),
                None => format!("anon_{i}"),
            };
            let field_type = &field["type"];
            let is_last = i == field_array.len() - 1;

            // Handle switch group: skip discriminator and switch fields,
            // emit the enum at the first switch field position.
            if let Some(ref sg) = switch_group {
                if field_name == sg.compare_to {
                    continue;
                }
                if sg.switch_indices.contains(&i) {
                    if i == sg.switch_indices[0] {
                        fields.push(ResolvedField {
                            name: to_snake_case(&sg.compare_to),
                            protocol_type: sg.enum_type.clone(),
                        });
                    }
                    continue;
                }
            }

            let protocol_type = self.resolve(field_type, parent_name, &field_name, is_last);

            if matches!(protocol_type, ProtocolType::Void) {
                continue;
            }

            fields.push(ResolvedField {
                name: to_snake_case(&field_name),
                protocol_type,
            });
        }

        fields
    }

    /// Resolves a JSON type definition into a `ProtocolType`.
    pub fn resolve(
        &self,
        type_def: &Value,
        parent_name: &str,
        field_name: &str,
        is_last: bool,
    ) -> ProtocolType {
        match type_def {
            Value::String(s) => self.resolve_string(s, parent_name, field_name, is_last),
            Value::Array(arr) if arr.len() == 2 => {
                self.resolve_compound(arr, parent_name, field_name, is_last)
            }
            _ => {
                eprintln!("Warning: unexpected type format, using Vec<u8>");
                ProtocolType::Opaque
            }
        }
    }

    /// Resolves a string type name (e.g., "varint", "string", "SpawnInfo").
    fn resolve_string(
        &self,
        name: &str,
        parent_name: &str,
        field_name: &str,
        is_last: bool,
    ) -> ProtocolType {
        match name {
            // Primitives
            "i8" => ProtocolType::I8,
            "i16" => ProtocolType::I16,
            "i32" => ProtocolType::I32,
            "i64" => ProtocolType::I64,
            "u8" => ProtocolType::U8,
            "u16" => ProtocolType::U16,
            "u32" => ProtocolType::U32,
            "u64" => ProtocolType::U64,
            "f32" => ProtocolType::F32,
            "f64" => ProtocolType::F64,
            "bool" => ProtocolType::Bool,
            "string" => ProtocolType::String,
            "UUID" => ProtocolType::Uuid,
            "position" => ProtocolType::Position,
            "Slot" => ProtocolType::Slot,
            "vec2f" => ProtocolType::Vec2f,
            "vec3f" => ProtocolType::Vec3f,
            "vec3f64" => ProtocolType::Vec3f64,
            "vec3i16" => ProtocolType::Vec3i16,
            // Wire-encoded
            "varint" | "ContainerID" | "optvarint" | "soundSource" => ProtocolType::VarInt,
            "varlong" => ProtocolType::VarLong,
            // NBT
            "anonymousNbt" => ProtocolType::NbtCompound,
            "anonOptionalNbt" => ProtocolType::OptionalNbt,
            // Byte arrays
            "ByteArray" => ProtocolType::Buffer(CountType::VarInt),
            // Packed position
            "packedChunkPos" => ProtocolType::I64,
            // Special
            "void" => ProtocolType::Void,
            "native" | "restBuffer" => {
                if is_last {
                    ProtocolType::Rest
                } else {
                    ProtocolType::Opaque
                }
            }
            // Custom type — resolve from registry
            other => self.resolve_custom(other, parent_name, field_name, is_last),
        }
    }

    /// Resolves a custom type name by looking it up in the registry.
    fn resolve_custom(
        &self,
        name: &str,
        parent_name: &str,
        field_name: &str,
        is_last: bool,
    ) -> ProtocolType {
        let resolved = self.types.get(name);
        match resolved {
            Some(value) if !value.is_null() => {
                if value.is_array() && value[0].as_str() == Some("container") {
                    let struct_name = format!("{}{}", parent_name, to_pascal_case(name));
                    let inner_fields = self.parse_container(value, &struct_name, false);
                    ProtocolType::InlineStruct {
                        name: struct_name,
                        fields: inner_fields,
                    }
                } else {
                    self.resolve(value, parent_name, field_name, is_last)
                }
            }
            _ => {
                eprintln!("Warning: unknown type '{name}', using Vec<u8>");
                ProtocolType::Opaque
            }
        }
    }

    /// Resolves a compound type (2-element JSON array like `["buffer", {...}]`).
    fn resolve_compound(
        &self,
        arr: &[Value],
        parent_name: &str,
        field_name: &str,
        is_last: bool,
    ) -> ProtocolType {
        let type_name = arr[0].as_str().unwrap_or("");
        match type_name {
            "buffer" => {
                let count_type = arr[1]["countType"].as_str().unwrap_or("varint");
                if count_type == "varint" {
                    ProtocolType::Buffer(CountType::VarInt)
                } else {
                    ProtocolType::Buffer(CountType::None)
                }
            }
            "option" => {
                let inner = self.resolve(&arr[1], parent_name, field_name, is_last);
                // Can't wrap inline structs in Option — fall back to opaque
                if matches!(inner, ProtocolType::InlineStruct { .. }) {
                    ProtocolType::Optional(Box::new(ProtocolType::Opaque))
                } else {
                    ProtocolType::Optional(Box::new(inner))
                }
            }
            "array" => {
                let count = if arr[1]["countType"].as_str().unwrap_or("varint") == "varint" {
                    CountType::VarInt
                } else {
                    CountType::None
                };
                let inner_type = &arr[1]["type"];

                if inner_type.is_array() && inner_type[0].as_str() == Some("container") {
                    let struct_name = format!("{}{}", parent_name, to_pascal_case(field_name));
                    let inner_fields = self.parse_container(inner_type, &struct_name, false);
                    ProtocolType::Array {
                        count,
                        inner: Box::new(ProtocolType::InlineStruct {
                            name: struct_name,
                            fields: inner_fields,
                        }),
                    }
                } else {
                    let inner = self.resolve(inner_type, parent_name, field_name, false);
                    ProtocolType::Array {
                        count,
                        inner: Box::new(inner),
                    }
                }
            }
            "switch" => ProtocolType::Opaque,
            "mapper" | "bitflags" => {
                let inner_type = &arr[1]["type"];
                self.resolve(inner_type, parent_name, field_name, is_last)
            }
            "container" => {
                let struct_name = format!("{}{}", parent_name, to_pascal_case(field_name));
                let inner_fields =
                    self.parse_container(&Value::Array(arr.to_vec()), &struct_name, false);
                ProtocolType::InlineStruct {
                    name: struct_name,
                    fields: inner_fields,
                }
            }
            "bitfield" => {
                let total_bits: u32 = arr[1]
                    .as_array()
                    .map(|fields| {
                        fields
                            .iter()
                            .filter_map(|f| f["size"].as_u64())
                            .sum::<u64>() as u32
                    })
                    .unwrap_or(32);
                ProtocolType::Bitfield(total_bits)
            }
            "registryEntryHolder"
            | "registryEntryHolderSet"
            | "topBitSetTerminatedArray"
            | "entityMetadataLoop" => {
                if is_last {
                    ProtocolType::Rest
                } else {
                    ProtocolType::Opaque
                }
            }
            _ => {
                eprintln!("Warning: unknown compound type '{type_name}', using Vec<u8>");
                ProtocolType::Opaque
            }
        }
    }

    /// Detects a group of switch fields sharing the same discriminator
    /// and builds a `SwitchEnum` type to replace them.
    fn detect_switch_group(&self, fields: &[Value], parent_name: &str) -> Option<SwitchGroup> {
        let switch_indices: Vec<usize> = fields
            .iter()
            .enumerate()
            .filter(|(_, f)| is_switch_field(f))
            .map(|(i, _)| i)
            .collect();

        if switch_indices.is_empty() {
            return None;
        }

        let mut compare_to: Option<String> = None;
        let mut switch_defs = Vec::new();

        for &idx in &switch_indices {
            let field = &fields[idx];
            let sw = &field["type"][1];
            let ct = sw["compareTo"].as_str()?;
            if ct.contains('/') {
                return None;
            }
            match &compare_to {
                None => compare_to = Some(ct.to_string()),
                Some(existing) if existing != ct => return None,
                _ => {}
            }
            let default = &sw["default"];
            if !default.is_null() && default.as_str() != Some("void") {
                return None;
            }
            switch_defs.push((field["name"].as_str()?.to_string(), sw));
        }

        let compare_to = compare_to?;

        // Collect variant fields by discriminator value
        let mut all_ids: BTreeMap<i32, Vec<ResolvedField>> = BTreeMap::new();
        for (field_name, sw) in &switch_defs {
            let sw_fields = sw["fields"].as_object()?;
            for (id_str, type_def) in sw_fields {
                let id: i32 = id_str.parse().ok()?;
                let pt = self.resolve(type_def, parent_name, field_name, false);
                if !matches!(pt, ProtocolType::Void) {
                    all_ids.entry(id).or_default().push(ResolvedField {
                        name: to_snake_case(field_name),
                        protocol_type: pt,
                    });
                }
            }
        }

        let enum_name = format!("{}{}", parent_name, to_pascal_case(&compare_to));
        let variants: Vec<SwitchVariant> = all_ids
            .iter()
            .map(|(&id, variant_fields)| SwitchVariant {
                id,
                name: format!("Variant{id}"),
                fields: variant_fields.clone(),
            })
            .collect();

        Some(SwitchGroup {
            compare_to,
            switch_indices,
            enum_type: ProtocolType::SwitchEnum {
                name: enum_name,
                variants,
            },
        })
    }
}

/// Intermediate result from analyzing a group of switch fields.
struct SwitchGroup {
    compare_to: String,
    switch_indices: Vec<usize>,
    enum_type: ProtocolType,
}

/// Returns true if a container field's type is a switch compound.
fn is_switch_field(field: &Value) -> bool {
    let field_type = &field["type"];
    field_type.is_array()
        && field_type
            .as_array()
            .is_some_and(|a| a.len() == 2 && a[0].as_str() == Some("switch"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_registry() -> TypeRegistry {
        TypeRegistry::new(&Value::Null, &Value::Null)
    }

    // -- resolve primitives --

    #[test]
    fn resolve_varint() {
        let reg = empty_registry();
        assert_eq!(
            reg.resolve(&Value::String("varint".into()), "", "", false),
            ProtocolType::VarInt
        );
    }

    #[test]
    fn resolve_string() {
        let reg = empty_registry();
        assert_eq!(
            reg.resolve(&Value::String("string".into()), "", "", false),
            ProtocolType::String
        );
    }

    #[test]
    fn resolve_uuid() {
        let reg = empty_registry();
        assert_eq!(
            reg.resolve(&Value::String("UUID".into()), "", "", false),
            ProtocolType::Uuid
        );
    }

    #[test]
    fn resolve_bool() {
        let reg = empty_registry();
        assert_eq!(
            reg.resolve(&Value::String("bool".into()), "", "", false),
            ProtocolType::Bool
        );
    }

    #[test]
    fn resolve_void() {
        let reg = empty_registry();
        assert_eq!(
            reg.resolve(&Value::String("void".into()), "", "", false),
            ProtocolType::Void
        );
    }

    #[test]
    fn resolve_rest_buffer_last() {
        let reg = empty_registry();
        assert_eq!(
            reg.resolve(&Value::String("restBuffer".into()), "", "", true),
            ProtocolType::Rest
        );
    }

    #[test]
    fn resolve_rest_buffer_not_last() {
        let reg = empty_registry();
        assert_eq!(
            reg.resolve(&Value::String("restBuffer".into()), "", "", false),
            ProtocolType::Opaque
        );
    }

    #[test]
    fn resolve_all_numeric_types() {
        let reg = empty_registry();
        for (json, expected) in [
            ("u8", ProtocolType::U8),
            ("u16", ProtocolType::U16),
            ("u32", ProtocolType::U32),
            ("u64", ProtocolType::U64),
            ("i8", ProtocolType::I8),
            ("i16", ProtocolType::I16),
            ("i32", ProtocolType::I32),
            ("i64", ProtocolType::I64),
            ("f32", ProtocolType::F32),
            ("f64", ProtocolType::F64),
        ] {
            assert_eq!(
                reg.resolve(&Value::String(json.into()), "", "", false),
                expected,
                "failed for {json}"
            );
        }
    }

    #[test]
    fn resolve_varint_aliases() {
        let reg = empty_registry();
        for alias in ["ContainerID", "optvarint", "soundSource"] {
            assert_eq!(
                reg.resolve(&Value::String(alias.into()), "", "", false),
                ProtocolType::VarInt,
                "failed for {alias}"
            );
        }
    }

    // -- resolve compounds --

    #[test]
    fn resolve_buffer_varint() {
        let reg = empty_registry();
        let json: Value = serde_json::from_str(r#"["buffer", {"countType": "varint"}]"#).unwrap();
        assert_eq!(
            reg.resolve(&json, "", "", false),
            ProtocolType::Buffer(CountType::VarInt)
        );
    }

    #[test]
    fn resolve_option_string() {
        let reg = empty_registry();
        let json: Value = serde_json::from_str(r#"["option", "string"]"#).unwrap();
        assert_eq!(
            reg.resolve(&json, "", "", false),
            ProtocolType::Optional(Box::new(ProtocolType::String))
        );
    }

    #[test]
    fn resolve_array_of_primitives() {
        let reg = empty_registry();
        let json: Value =
            serde_json::from_str(r#"["array", {"countType": "varint", "type": "i32"}]"#).unwrap();
        assert_eq!(
            reg.resolve(&json, "", "", false),
            ProtocolType::Array {
                count: CountType::VarInt,
                inner: Box::new(ProtocolType::I32)
            }
        );
    }

    #[test]
    fn resolve_varlong() {
        let reg = empty_registry();
        assert_eq!(
            reg.resolve(&Value::String("varlong".into()), "", "", false),
            ProtocolType::VarLong
        );
    }

    #[test]
    fn resolve_unknown_falls_back() {
        let reg = empty_registry();
        assert_eq!(
            reg.resolve(&Value::String("unknownType".into()), "", "", false),
            ProtocolType::Opaque
        );
    }

    #[test]
    fn resolve_position() {
        let reg = empty_registry();
        assert_eq!(
            reg.resolve(&Value::String("position".into()), "", "", false),
            ProtocolType::Position
        );
    }

    #[test]
    fn resolve_nbt() {
        let reg = empty_registry();
        assert_eq!(
            reg.resolve(&Value::String("anonymousNbt".into()), "", "", false),
            ProtocolType::NbtCompound
        );
    }

    #[test]
    fn resolve_optional_nbt() {
        let reg = empty_registry();
        assert_eq!(
            reg.resolve(&Value::String("anonOptionalNbt".into()), "", "", false),
            ProtocolType::OptionalNbt
        );
    }

    #[test]
    fn resolve_byte_array() {
        let reg = empty_registry();
        assert_eq!(
            reg.resolve(&Value::String("ByteArray".into()), "", "", false),
            ProtocolType::Buffer(CountType::VarInt)
        );
    }

    #[test]
    fn resolve_slot() {
        let reg = empty_registry();
        assert_eq!(
            reg.resolve(&Value::String("Slot".into()), "", "", false),
            ProtocolType::Slot
        );
    }

    #[test]
    fn resolve_vector_types() {
        let reg = empty_registry();
        for (json, expected) in [
            ("vec2f", ProtocolType::Vec2f),
            ("vec3f", ProtocolType::Vec3f),
            ("vec3f64", ProtocolType::Vec3f64),
            ("vec3i16", ProtocolType::Vec3i16),
        ] {
            assert_eq!(
                reg.resolve(&Value::String(json.into()), "", "", false),
                expected,
                "failed for {json}"
            );
        }
    }

    #[test]
    fn resolve_packed_chunk_pos() {
        let reg = empty_registry();
        assert_eq!(
            reg.resolve(&Value::String("packedChunkPos".into()), "", "", false),
            ProtocolType::I64
        );
    }

    #[test]
    fn resolve_switch_returns_opaque() {
        let reg = empty_registry();
        let json: Value = serde_json::from_str(
            r#"["switch", {"compareTo": "action", "fields": {"0": "string"}, "default": "void"}]"#,
        )
        .unwrap();
        assert_eq!(reg.resolve(&json, "", "", false), ProtocolType::Opaque);
    }

    // -- parse_container --

    #[test]
    fn parse_simple_container() {
        let reg = empty_registry();
        let json: Value = serde_json::from_str(
            r#"["container", [
                {"name": "username", "type": "string"},
                {"name": "age", "type": "varint"}
            ]]"#,
        )
        .unwrap();
        let fields = reg.parse_container(&json, "TestPacket", false);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "username");
        assert_eq!(fields[0].protocol_type, ProtocolType::String);
        assert_eq!(fields[1].name, "age");
        assert_eq!(fields[1].protocol_type, ProtocolType::VarInt);
    }

    #[test]
    fn parse_container_with_rest_buffer() {
        let reg = empty_registry();
        let json: Value = serde_json::from_str(
            r#"["container", [
                {"name": "id", "type": "varint"},
                {"name": "data", "type": "restBuffer"}
            ]]"#,
        )
        .unwrap();
        let fields = reg.parse_container(&json, "TestPacket", false);
        assert_eq!(fields[1].name, "data");
        assert_eq!(fields[1].protocol_type, ProtocolType::Rest);
    }

    #[test]
    fn parse_container_with_inline_struct() {
        let reg = empty_registry();
        let json: Value = serde_json::from_str(
            r#"["container", [
                {"name": "items", "type": ["array", {
                    "countType": "varint",
                    "type": ["container", [
                        {"name": "name", "type": "string"},
                        {"name": "value", "type": "i32"}
                    ]]
                }]}
            ]]"#,
        )
        .unwrap();
        let fields = reg.parse_container(&json, "TestPacket", false);
        assert_eq!(fields.len(), 1);
        match &fields[0].protocol_type {
            ProtocolType::Array { count, inner } => {
                assert_eq!(*count, CountType::VarInt);
                match inner.as_ref() {
                    ProtocolType::InlineStruct { name, fields } => {
                        assert_eq!(name, "TestPacketItems");
                        assert_eq!(fields.len(), 2);
                    }
                    _ => panic!("expected InlineStruct"),
                }
            }
            _ => panic!("expected Array"),
        }
    }

    #[test]
    fn parse_empty_container() {
        let reg = empty_registry();
        let json: Value = serde_json::from_str(r#"["container", []]"#).unwrap();
        let fields = reg.parse_container(&json, "TestPacket", false);
        assert!(fields.is_empty());
    }

    // -- switch enums --

    #[test]
    fn trailing_switches_generate_enum() {
        let reg = empty_registry();
        let json: Value = serde_json::from_str(
            r#"["container", [
                {"name": "target", "type": "varint"},
                {"name": "action", "type": "varint"},
                {"name": "x", "type": ["switch", {"compareTo": "action", "fields": {"2": "f32"}, "default": "void"}]},
                {"name": "hand", "type": ["switch", {"compareTo": "action", "fields": {"0": "varint", "2": "varint"}, "default": "void"}]}
            ]]"#,
        )
        .unwrap();
        let fields = reg.parse_container(&json, "Test", true);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "target");
        assert_eq!(fields[1].name, "action");
        match &fields[1].protocol_type {
            ProtocolType::SwitchEnum { name, variants } => {
                assert_eq!(name, "TestAction");
                assert!(variants.len() >= 2);
            }
            _ => panic!("expected SwitchEnum"),
        }
    }

    #[test]
    fn interleaved_switch_generates_enum() {
        let reg = empty_registry();
        let json: Value = serde_json::from_str(
            r#"["container", [
                {"name": "action", "type": "varint"},
                {"name": "data", "type": ["switch", {"compareTo": "action", "fields": {"0": "string"}, "default": "void"}]},
                {"name": "sneaking", "type": "bool"}
            ]]"#,
        )
        .unwrap();
        let fields = reg.parse_container(&json, "Test", true);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "action");
        assert!(matches!(
            fields[0].protocol_type,
            ProtocolType::SwitchEnum { .. }
        ));
        assert_eq!(fields[1].name, "sneaking");
        assert_eq!(fields[1].protocol_type, ProtocolType::Bool);
    }

    #[test]
    fn single_trailing_switch_generates_enum() {
        let reg = empty_registry();
        let json: Value = serde_json::from_str(
            r#"["container", [
                {"name": "id", "type": "varint"},
                {"name": "data", "type": ["switch", {"compareTo": "id", "fields": {"1": "string"}, "default": "void"}]}
            ]]"#,
        )
        .unwrap();
        let fields = reg.parse_container(&json, "Test", true);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "id");
        match &fields[0].protocol_type {
            ProtocolType::SwitchEnum { name, variants } => {
                assert_eq!(name, "TestId");
                assert_eq!(variants.len(), 1);
                assert_eq!(variants[0].id, 1);
            }
            _ => panic!("expected SwitchEnum"),
        }
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

        let reg = TypeRegistry::new(&json["toServer"]["types"], &Value::Null);
        let packets = reg.parse_direction(&json, "toServer", "Serverbound", "Login");
        assert_eq!(packets.len(), 1);
        assert!(packets[0].name.contains("LoginStart"));
    }

    #[test]
    fn parse_direction_empty() {
        let json: Value = serde_json::from_str(r#"{}"#).unwrap();
        let reg = TypeRegistry::new(&Value::Null, &Value::Null);
        let packets = reg.parse_direction(&json, "toServer", "Serverbound", "Login");
        assert!(packets.is_empty());
    }
}
