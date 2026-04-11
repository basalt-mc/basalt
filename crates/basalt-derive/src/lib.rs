//! Proc-macro crate for deriving `Encode`, `Decode`, and `EncodedSize` traits.
//!
//! Generates serialization implementations for Minecraft protocol structs
//! and enums. The generated code references `basalt_types` — consumer crates
//! must depend on `basalt-types` alongside `basalt-derive`.
//!
//! # Struct attributes
//!
//! - `#[packet(id = 0x00)]` — associates a packet ID, encoded as VarInt before fields
//!
//! # Field attributes
//!
//! - `#[field(varint)]` — encode i32/i64 as VarInt/VarLong
//! - `#[field(length = "varint")]` — VarInt length prefix for Vec
//! - `#[field(optional)]` — boolean-prefixed Option
//! - `#[field(rest)]` — consume remaining bytes (last field only, must be Vec<u8>)
//!
//! # Enum attributes
//!
//! - `#[variant(id = N)]` — explicit discriminant (default: sequential from 0)

mod attrs;
mod decode;
mod encode;
mod size;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

/// Derives the `Encode` trait for a struct or enum.
///
/// For structs, encodes each field in declaration order. If `#[packet(id)]`
/// is present, a VarInt packet ID is written first. Field attributes control
/// encoding behavior (varint, optional, length-prefixed, rest).
///
/// For enums, writes a VarInt discriminant followed by the variant's fields.
/// Discriminants are sequential from 0 unless overridden with `#[variant(id)]`.
#[proc_macro_derive(Encode, attributes(packet, field, variant))]
pub fn derive_encode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match encode::derive_encode(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derives the `Decode` trait for a struct or enum.
///
/// For structs, decodes each field in declaration order. If `#[packet(id)]`
/// is present, a VarInt packet ID is read and validated first. Field
/// attributes control decoding behavior.
///
/// For enums, reads a VarInt discriminant and decodes the matching variant.
/// Returns `Error::InvalidData` for unknown discriminants.
#[proc_macro_derive(Decode, attributes(packet, field, variant))]
pub fn derive_decode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match decode::derive_decode(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derives the `EncodedSize` trait for a struct or enum.
///
/// For structs, sums the encoded size of each field. If `#[packet(id)]`
/// is present, includes the VarInt packet ID size. Field attributes
/// are accounted for (varint size, optional prefix byte, length prefix).
///
/// For enums, returns the VarInt discriminant size plus the matched
/// variant's fields size.
#[proc_macro_derive(EncodedSize, attributes(packet, field, variant))]
pub fn derive_encoded_size(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match size::derive_encoded_size(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
