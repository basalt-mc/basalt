//! Proc-macro crate for Minecraft protocol serialization.
//!
//! Provides two ways to generate `Encode`, `Decode`, and `EncodedSize` impls:
//!
//! ## `#[packet(id = N)]` — for protocol packets
//!
//! Attribute macro that generates all three trait impls plus a `PACKET_ID`
//! constant. Use this on every packet struct:
//!
//! ```ignore
//! #[derive(Debug, Clone, PartialEq)]
//! #[packet(id = 0x00)]
//! pub struct StatusRequest;
//! ```
//!
//! ## `#[derive(Encode, Decode, EncodedSize)]` — for non-packet types
//!
//! Standard derive macros for inner data structures, enums, and other types
//! that need serialization but aren't protocol packets:
//!
//! ```ignore
//! #[derive(Debug, Encode, Decode, EncodedSize)]
//! pub struct SomeInnerData {
//!     pub value: i32,
//! }
//! ```
//!
//! ## Field attributes
//!
//! - `#[field(varint)]` — encode i32/i64 as VarInt/VarLong
//! - `#[field(length = "varint")]` — VarInt length prefix for Vec
//! - `#[field(optional)]` — boolean-prefixed Option
//! - `#[field(rest)]` — consume remaining bytes (last field only, must be Vec<u8>)
//!
//! ## Enum attributes
//!
//! - `#[variant(id = N)]` — explicit discriminant (default: sequential from 0)

mod attrs;
mod decode;
mod encode;
mod packet;
mod size;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

/// Parses the `#[packet(id = N)]` attribute arguments.
///
/// Handles both positive (`id = 0x00`) and negative (`id = -1`) integer
/// literals. The attribute arguments are passed separately from the item
/// by the proc macro framework.
struct PacketAttrArgs {
    id: i32,
}

impl syn::parse::Parse for PacketAttrArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let ident: syn::Ident = input.parse()?;
        if ident != "id" {
            return Err(syn::Error::new(ident.span(), "expected `id`"));
        }
        let _eq: syn::Token![=] = input.parse()?;

        // Handle negative literals (unary minus + int literal)
        let negative = input.peek(syn::Token![-]);
        if negative {
            let _neg: syn::Token![-] = input.parse()?;
            let lit: syn::LitInt = input.parse()?;
            Ok(Self {
                id: -(lit.base10_parse::<i32>()?),
            })
        } else {
            let lit: syn::LitInt = input.parse()?;
            Ok(Self {
                id: lit.base10_parse::<i32>()?,
            })
        }
    }
}

/// Attribute macro for protocol packet structs.
///
/// Generates `Encode`, `Decode`, `EncodedSize` implementations and a
/// `pub const PACKET_ID: i32` associated constant. The packet ID is NOT
/// included in the wire format — it is only a declarative constant used
/// by the packet registry for dispatch.
///
/// This replaces the need to manually derive all three traits on packet
/// structs. Field attributes (`#[field(varint)]`, etc.) work the same
/// as with the individual derives.
///
/// # Example
///
/// ```ignore
/// #[derive(Debug, Clone, PartialEq)]
/// #[packet(id = 0x00)]
/// pub struct HandshakePacket {
///     #[field(varint)]
///     pub protocol_version: i32,
///     pub server_address: String,
///     pub server_port: u16,
///     #[field(varint)]
///     pub next_state: i32,
/// }
///
/// assert_eq!(HandshakePacket::PACKET_ID, 0x00);
/// ```
#[proc_macro_attribute]
pub fn packet(attr: TokenStream, item: TokenStream) -> TokenStream {
    // Parse "id = N" from the attribute arguments
    let packet_id = match syn::parse::<PacketAttrArgs>(attr) {
        Ok(args) => args.id,
        Err(err) => return err.to_compile_error().into(),
    };
    let input = parse_macro_input!(item as DeriveInput);
    match packet::expand_packet(&input, packet_id) {
        Ok(tokens) => {
            // Re-emit the original struct definition + generated impls
            let name = &input.ident;
            let vis = &input.vis;
            let attrs: Vec<_> = input
                .attrs
                .iter()
                .filter(|a| !a.path().is_ident("packet"))
                .collect();
            let generics = &input.generics;

            let struct_def = match &input.data {
                syn::Data::Struct(data) => match &data.fields {
                    syn::Fields::Named(fields) => {
                        // Re-emit fields without #[field(...)] attributes
                        let clean_fields: Vec<_> = fields
                            .named
                            .iter()
                            .map(|f| {
                                let field_attrs: Vec<_> = f
                                    .attrs
                                    .iter()
                                    .filter(|a| !a.path().is_ident("field"))
                                    .collect();
                                let vis = &f.vis;
                                let name = &f.ident;
                                let ty = &f.ty;
                                quote::quote! {
                                    #(#field_attrs)*
                                    #vis #name: #ty
                                }
                            })
                            .collect();
                        quote::quote! {
                            #(#attrs)*
                            #vis struct #name #generics {
                                #(#clean_fields),*
                            }
                        }
                    }
                    syn::Fields::Unit => {
                        quote::quote! {
                            #(#attrs)*
                            #vis struct #name #generics;
                        }
                    }
                    syn::Fields::Unnamed(fields) => {
                        let fields = &fields.unnamed;
                        quote::quote! {
                            #(#attrs)*
                            #vis struct #name #generics(#fields);
                        }
                    }
                },
                _ => {
                    return syn::Error::new_spanned(input, "#[packet] can only be used on structs")
                        .to_compile_error()
                        .into();
                }
            };

            let combined = quote::quote! {
                #struct_def
                #tokens
            };
            combined.into()
        }
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derives the `Encode` trait for a struct or enum.
///
/// For structs, encodes each field in declaration order. Field attributes
/// control encoding behavior (varint, optional, length-prefixed, rest).
///
/// For enums, writes a VarInt discriminant followed by the variant's fields.
/// Discriminants are sequential from 0 unless overridden with `#[variant(id)]`.
///
/// **For packet structs, use `#[packet(id = N)]` instead** — it generates
/// Encode, Decode, EncodedSize, and PACKET_ID all at once.
#[proc_macro_derive(Encode, attributes(field, variant))]
pub fn derive_encode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match encode::derive_encode(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derives the `Decode` trait for a struct or enum.
///
/// For structs, decodes each field in declaration order.
///
/// For enums, reads a VarInt discriminant and decodes the matching variant.
/// Returns `Error::InvalidData` for unknown discriminants.
///
/// **For packet structs, use `#[packet(id = N)]` instead.**
#[proc_macro_derive(Decode, attributes(field, variant))]
pub fn derive_decode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match decode::derive_decode(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derives the `EncodedSize` trait for a struct or enum.
///
/// For structs, sums the encoded size of each field.
///
/// For enums, returns the VarInt discriminant size plus the matched
/// variant's fields size.
///
/// **For packet structs, use `#[packet(id = N)]` instead.**
#[proc_macro_derive(EncodedSize, attributes(field, variant))]
pub fn derive_encoded_size(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match size::derive_encoded_size(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
