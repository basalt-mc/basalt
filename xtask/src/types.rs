//! Data structures used across the codegen pipeline.
//!
//! These types represent parsed protocol packet definitions and are
//! shared between the parser, code generator, and play-split modules.

/// A parsed packet definition ready for code generation.
///
/// Contains the Rust struct name, the protocol packet ID, all fields,
/// and any inline structs that were generated from container types
/// embedded in the packet definition (e.g., arrays of containers).
#[derive(Debug)]
pub(crate) struct PacketDef {
    /// Fully qualified Rust struct name (e.g., "ServerboundLoginLoginStart").
    pub name: String,
    /// Protocol packet ID as a hex string (e.g., "0x00").
    pub id: String,
    /// Ordered list of fields in the packet struct.
    pub fields: Vec<FieldDef>,
    /// Inline structs generated from embedded container types.
    pub inline_structs: Vec<InlineStruct>,
    /// Inline enums generated from switch field groups.
    pub switch_enums: Vec<SwitchEnum>,
}

/// A single field in a packet or inline struct.
///
/// The `attribute` maps to a `#[field(...)]` derive attribute when
/// present — used for VarInt encoding, length-prefixed arrays,
/// optional fields, and rest buffers.
#[derive(Debug, Clone)]
pub(crate) struct FieldDef {
    /// Rust field name in snake_case.
    pub name: String,
    /// Rust type as a string (e.g., "i32", "Vec<u8>", "String").
    pub rust_type: String,
    /// Optional `#[field(...)]` attribute content (e.g., "varint", "length = \"varint\"").
    pub attribute: Option<String>,
}

/// An inline struct generated from a container type embedded in a
/// packet field definition.
///
/// These are emitted as separate `#[derive(Encode, Decode, EncodedSize)]`
/// structs before the parent packet struct, and referenced by name
/// in the parent's field type (e.g., `Vec<LoginSuccessProperties>`).
#[derive(Debug)]
pub(crate) struct InlineStruct {
    /// Rust struct name in PascalCase.
    pub name: String,
    /// Ordered list of fields.
    pub fields: Vec<FieldDef>,
}

/// An inline enum generated from a group of switch fields that share
/// the same `compareTo` discriminator.
///
/// Each variant corresponds to one discriminator value and contains
/// the fields that are present for that value. Emitted as a separate
/// `#[derive(Encode, Decode, EncodedSize)]` enum before the parent
/// packet struct.
#[derive(Debug, Clone)]
pub(crate) struct SwitchEnum {
    /// Rust enum name in PascalCase.
    pub name: String,
    /// Ordered list of variants, one per discriminator value.
    pub variants: Vec<SwitchVariant>,
}

/// A single variant in a `SwitchEnum`.
#[derive(Debug, Clone)]
pub(crate) struct SwitchVariant {
    /// Discriminator value (e.g., 0, 1, 2).
    pub id: i32,
    /// Rust variant name in PascalCase (e.g., "Variant0").
    pub variant_name: String,
    /// Fields present in this variant (empty for unit variants).
    pub fields: Vec<FieldDef>,
}
