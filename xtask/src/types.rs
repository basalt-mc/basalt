//! Intermediate representation for the codegen pipeline.
//!
//! Defines `ProtocolType` — a typed representation of Minecraft protocol
//! types that sits between the raw JSON definitions and the generated
//! Rust source code. The pipeline is:
//!
//! ```text
//! JSON ──► TypeRegistry ──► ProtocolType (IR) ──► Rust code
//! ```

/// How a variable-length collection's count is encoded on the wire.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CountType {
    /// Length encoded as a VarInt before the elements.
    VarInt,
    /// No length prefix — count is implied by context.
    None,
}

/// A resolved protocol type, independent of both JSON representation
/// and Rust output. Every protocol field maps to exactly one variant.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ProtocolType {
    // -- Primitives --
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
    String,
    Uuid,
    Position,
    NbtCompound,
    OptionalNbt,
    Slot,
    Vec2f,
    Vec3f,
    Vec3f64,
    Vec3i16,

    // -- Wire-encoded integers --
    /// i32 encoded as a VarInt (1-5 bytes).
    VarInt,
    /// i64 encoded as a VarLong (1-10 bytes).
    VarLong,

    // -- Composite --
    /// Length-prefixed or unbounded array of a single element type.
    Array {
        count: CountType,
        inner: Box<ProtocolType>,
    },
    /// Boolean-prefixed optional value.
    Optional(Box<ProtocolType>),
    /// Length-prefixed raw byte buffer.
    Buffer(CountType),
    /// All remaining bytes in the packet (must be the last field).
    Rest,

    // -- Structures --
    /// An inline struct generated from a container definition.
    InlineStruct {
        name: std::string::String,
        fields: Vec<ResolvedField>,
    },
    /// An enum generated from a group of switch fields.
    SwitchEnum {
        name: std::string::String,
        variants: Vec<SwitchVariant>,
    },

    // -- Special --
    /// Packed bit fields — total bit count determines the integer type.
    Bitfield(u32),
    /// Opaque bytes for native types we can't parse (entityMetadata, etc.).
    Opaque,
    /// No data on the wire — the field should be filtered out.
    Void,
}

/// A resolved field ready for code generation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedField {
    /// Field name in snake_case.
    pub name: std::string::String,
    /// The resolved protocol type.
    pub protocol_type: ProtocolType,
}

/// A single variant in a switch enum.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SwitchVariant {
    /// Discriminator value (e.g., 0, 1, 2).
    pub id: i32,
    /// Rust variant name in PascalCase (e.g., "Variant0").
    pub name: std::string::String,
    /// Fields present in this variant (empty for unit variants).
    pub fields: Vec<ResolvedField>,
}

/// A parsed packet definition ready for code generation.
#[derive(Debug)]
pub(crate) struct PacketDef {
    /// Fully qualified Rust struct name (e.g., "ServerboundLoginLoginStart").
    pub name: std::string::String,
    /// Protocol packet ID as a hex string (e.g., "0x00").
    pub id: std::string::String,
    /// Ordered list of resolved fields.
    pub fields: Vec<ResolvedField>,
}

impl ProtocolType {
    /// Converts this IR type to its Rust type string and optional
    /// `#[field(...)]` attribute for the derive macros.
    pub fn to_rust(&self) -> (std::string::String, Option<std::string::String>) {
        match self {
            Self::I8 => ("i8".into(), None),
            Self::I16 => ("i16".into(), None),
            Self::I32 => ("i32".into(), None),
            Self::I64 => ("i64".into(), None),
            Self::U8 => ("u8".into(), None),
            Self::U16 => ("u16".into(), None),
            Self::U32 => ("u32".into(), None),
            Self::U64 => ("u64".into(), None),
            Self::F32 => ("f32".into(), None),
            Self::F64 => ("f64".into(), None),
            Self::Bool => ("bool".into(), None),
            Self::String => ("String".into(), None),
            Self::Uuid => ("Uuid".into(), None),
            Self::Position => ("Position".into(), None),
            Self::NbtCompound => ("NbtCompound".into(), None),
            Self::OptionalNbt => ("Option<NbtCompound>".into(), Some("optional".into())),
            Self::Slot => ("Slot".into(), None),
            Self::Vec2f => ("Vec2f".into(), None),
            Self::Vec3f => ("Vec3f".into(), None),
            Self::Vec3f64 => ("Vec3f64".into(), None),
            Self::Vec3i16 => ("Vec3i16".into(), None),
            Self::VarInt => ("i32".into(), Some("varint".into())),
            Self::VarLong => ("i64".into(), Some("varlong".into())),
            Self::Array { inner, .. } => {
                let (inner_rust, inner_attr) = inner.to_rust();
                // All protocol arrays use a VarInt length prefix.
                // Vec<T> has no blanket Encode/Decode impl without it.
                // Nested arrays (Vec<Vec<T>>) can't work because the
                // inner Vec also needs a length attribute — fall back
                // to Vec<Vec<u8>>.
                if inner_attr.is_some()
                    && !matches!(inner.as_ref(), ProtocolType::InlineStruct { .. })
                {
                    ("Vec<Vec<u8>>".into(), Some("length = \"varint\"".into()))
                } else {
                    (
                        format!("Vec<{inner_rust}>"),
                        Some("length = \"varint\"".into()),
                    )
                }
            }
            Self::Optional(inner) => {
                // When the inner type requires its own attribute (e.g.,
                // Array needs length="varint"), we can't stack two
                // attributes. Fall back to Option<Vec<u8>> for complex
                // inner types.
                let (inner_rust, inner_attr) = inner.to_rust();
                if inner_attr.is_some() {
                    ("Option<Vec<u8>>".into(), Some("optional".into()))
                } else {
                    (format!("Option<{inner_rust}>"), Some("optional".into()))
                }
            }
            Self::Buffer(count) => {
                let attr = match count {
                    CountType::VarInt => Some("length = \"varint\"".into()),
                    CountType::None => None,
                };
                ("Vec<u8>".into(), attr)
            }
            Self::Rest => ("Vec<u8>".into(), Some("rest".into())),
            Self::InlineStruct { name, .. } => (name.clone(), None),
            Self::SwitchEnum { name, .. } => (name.clone(), None),
            Self::Bitfield(bits) => {
                let ty = match bits {
                    0..=8 => "u8",
                    9..=16 => "u16",
                    17..=32 => "u32",
                    _ => "u64",
                };
                (ty.into(), None)
            }
            Self::Opaque => ("Vec<u8>".into(), None),
            Self::Void => ("__void__".into(), None),
        }
    }

    /// Returns true if this type needs `Encode`/`Decode`/`EncodedSize`
    /// derive imports (inline structs and switch enums).
    pub fn needs_derive_imports(&self) -> bool {
        matches!(self, Self::InlineStruct { .. } | Self::SwitchEnum { .. })
    }

    /// Collects the basalt-types import name if this type needs one.
    pub fn basalt_import(&self) -> Option<&'static str> {
        match self {
            Self::Uuid => Some("Uuid"),
            Self::Position => Some("Position"),
            Self::NbtCompound | Self::OptionalNbt => Some("NbtCompound"),
            Self::Slot => Some("Slot"),
            Self::Vec2f => Some("Vec2f"),
            Self::Vec3f => Some("Vec3f"),
            Self::Vec3f64 => Some("Vec3f64"),
            Self::Vec3i16 => Some("Vec3i16"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_rust_primitives() {
        assert_eq!(ProtocolType::I8.to_rust(), ("i8".into(), None));
        assert_eq!(ProtocolType::I16.to_rust(), ("i16".into(), None));
        assert_eq!(ProtocolType::I32.to_rust(), ("i32".into(), None));
        assert_eq!(ProtocolType::I64.to_rust(), ("i64".into(), None));
        assert_eq!(ProtocolType::U8.to_rust(), ("u8".into(), None));
        assert_eq!(ProtocolType::U16.to_rust(), ("u16".into(), None));
        assert_eq!(ProtocolType::U32.to_rust(), ("u32".into(), None));
        assert_eq!(ProtocolType::U64.to_rust(), ("u64".into(), None));
        assert_eq!(ProtocolType::F32.to_rust(), ("f32".into(), None));
        assert_eq!(ProtocolType::F64.to_rust(), ("f64".into(), None));
        assert_eq!(ProtocolType::Bool.to_rust(), ("bool".into(), None));
        assert_eq!(ProtocolType::String.to_rust(), ("String".into(), None));
        assert_eq!(ProtocolType::Uuid.to_rust(), ("Uuid".into(), None));
        assert_eq!(ProtocolType::Position.to_rust(), ("Position".into(), None));
        assert_eq!(ProtocolType::Slot.to_rust(), ("Slot".into(), None));
        assert_eq!(ProtocolType::Vec2f.to_rust(), ("Vec2f".into(), None));
        assert_eq!(ProtocolType::Vec3f.to_rust(), ("Vec3f".into(), None));
        assert_eq!(ProtocolType::Vec3f64.to_rust(), ("Vec3f64".into(), None));
        assert_eq!(ProtocolType::Vec3i16.to_rust(), ("Vec3i16".into(), None));
    }

    #[test]
    fn to_rust_nbt() {
        assert_eq!(
            ProtocolType::NbtCompound.to_rust(),
            ("NbtCompound".into(), None)
        );
        assert_eq!(
            ProtocolType::OptionalNbt.to_rust(),
            ("Option<NbtCompound>".into(), Some("optional".into()))
        );
    }

    #[test]
    fn to_rust_wire_encoded() {
        assert_eq!(
            ProtocolType::VarInt.to_rust(),
            ("i32".into(), Some("varint".into()))
        );
        assert_eq!(
            ProtocolType::VarLong.to_rust(),
            ("i64".into(), Some("varlong".into()))
        );
    }

    #[test]
    fn to_rust_array() {
        let pt = ProtocolType::Array {
            count: CountType::VarInt,
            inner: Box::new(ProtocolType::I32),
        };
        assert_eq!(
            pt.to_rust(),
            ("Vec<i32>".into(), Some("length = \"varint\"".into()))
        );
    }

    #[test]
    fn to_rust_nested_array_falls_back() {
        let pt = ProtocolType::Array {
            count: CountType::VarInt,
            inner: Box::new(ProtocolType::Array {
                count: CountType::VarInt,
                inner: Box::new(ProtocolType::String),
            }),
        };
        assert_eq!(
            pt.to_rust(),
            ("Vec<Vec<u8>>".into(), Some("length = \"varint\"".into()))
        );
    }

    #[test]
    fn to_rust_optional() {
        let pt = ProtocolType::Optional(Box::new(ProtocolType::String));
        assert_eq!(
            pt.to_rust(),
            ("Option<String>".into(), Some("optional".into()))
        );
    }

    #[test]
    fn to_rust_optional_with_complex_inner_falls_back() {
        let pt = ProtocolType::Optional(Box::new(ProtocolType::Array {
            count: CountType::VarInt,
            inner: Box::new(ProtocolType::I32),
        }));
        assert_eq!(
            pt.to_rust(),
            ("Option<Vec<u8>>".into(), Some("optional".into()))
        );
    }

    #[test]
    fn to_rust_buffer() {
        assert_eq!(
            ProtocolType::Buffer(CountType::VarInt).to_rust(),
            ("Vec<u8>".into(), Some("length = \"varint\"".into()))
        );
        assert_eq!(
            ProtocolType::Buffer(CountType::None).to_rust(),
            ("Vec<u8>".into(), None)
        );
    }

    #[test]
    fn to_rust_rest() {
        assert_eq!(
            ProtocolType::Rest.to_rust(),
            ("Vec<u8>".into(), Some("rest".into()))
        );
    }

    #[test]
    fn to_rust_bitfield() {
        assert_eq!(ProtocolType::Bitfield(8).to_rust(), ("u8".into(), None));
        assert_eq!(ProtocolType::Bitfield(16).to_rust(), ("u16".into(), None));
        assert_eq!(ProtocolType::Bitfield(32).to_rust(), ("u32".into(), None));
        assert_eq!(ProtocolType::Bitfield(64).to_rust(), ("u64".into(), None));
    }

    #[test]
    fn to_rust_opaque_and_void() {
        assert_eq!(ProtocolType::Opaque.to_rust(), ("Vec<u8>".into(), None));
        assert_eq!(ProtocolType::Void.to_rust(), ("__void__".into(), None));
    }

    #[test]
    fn to_rust_inline_struct() {
        let pt = ProtocolType::InlineStruct {
            name: "MyStruct".into(),
            fields: vec![],
        };
        assert_eq!(pt.to_rust(), ("MyStruct".into(), None));
    }

    #[test]
    fn to_rust_switch_enum() {
        let pt = ProtocolType::SwitchEnum {
            name: "MyEnum".into(),
            variants: vec![],
        };
        assert_eq!(pt.to_rust(), ("MyEnum".into(), None));
    }

    #[test]
    fn needs_derive_imports_check() {
        assert!(
            ProtocolType::InlineStruct {
                name: "X".into(),
                fields: vec![]
            }
            .needs_derive_imports()
        );
        assert!(
            ProtocolType::SwitchEnum {
                name: "X".into(),
                variants: vec![]
            }
            .needs_derive_imports()
        );
        assert!(!ProtocolType::I32.needs_derive_imports());
    }

    #[test]
    fn basalt_import_check() {
        assert_eq!(ProtocolType::Uuid.basalt_import(), Some("Uuid"));
        assert_eq!(ProtocolType::Position.basalt_import(), Some("Position"));
        assert_eq!(
            ProtocolType::NbtCompound.basalt_import(),
            Some("NbtCompound")
        );
        assert_eq!(
            ProtocolType::OptionalNbt.basalt_import(),
            Some("NbtCompound")
        );
        assert_eq!(ProtocolType::Slot.basalt_import(), Some("Slot"));
        assert_eq!(ProtocolType::I32.basalt_import(), None);
        assert_eq!(ProtocolType::VarInt.basalt_import(), None);
    }
}
