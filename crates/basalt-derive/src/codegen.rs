//! Shared code generation helpers for field attribute handling.
//!
//! These functions generate `TokenStream` fragments for a single field
//! based on its `FieldAttr`. They are used by both struct and enum
//! codegen paths, eliminating the duplication between the two.
//!
//! The `val` parameter is the expression that accesses the field value:
//! - For structs: `self.field_name`
//! - For enum variants: `field_name` (destructured binding)

use proc_macro2::TokenStream;
use quote::quote;

use crate::attrs::FieldAttr;

/// Generates the encode expression for a single field.
pub fn field_encode(val: &TokenStream, attr: &FieldAttr) -> TokenStream {
    if attr.varint {
        quote! { basalt_types::Encode::encode(&basalt_types::VarInt(*#val), buf)?; }
    } else if attr.optional {
        quote! {
            match #val {
                Some(value) => {
                    basalt_types::Encode::encode(&true, buf)?;
                    basalt_types::Encode::encode(value, buf)?;
                }
                None => {
                    basalt_types::Encode::encode(&false, buf)?;
                }
            }
        }
    } else if attr.length_varint && attr.element_varint {
        quote! {
            basalt_types::Encode::encode(&basalt_types::VarInt((#val).len() as i32), buf)?;
            for item in (#val) {
                basalt_types::Encode::encode(&basalt_types::VarInt(*item), buf)?;
            }
        }
    } else if attr.length_varint {
        quote! {
            basalt_types::Encode::encode(&basalt_types::VarInt((#val).len() as i32), buf)?;
            for item in (#val) {
                basalt_types::Encode::encode(item, buf)?;
            }
        }
    } else if attr.rest {
        quote! { buf.extend_from_slice(#val); }
    } else {
        quote! { basalt_types::Encode::encode(#val, buf)?; }
    }
}

/// Generates the decode expression for a single field.
///
/// `field_name` is the local variable name to bind the decoded value.
/// `field_type` is the type (only used for the default decode path).
pub fn field_decode(
    field_name: &syn::Ident,
    field_type: &syn::Type,
    attr: &FieldAttr,
) -> TokenStream {
    if attr.varint {
        quote! {
            let #field_name = {
                let var: basalt_types::VarInt = basalt_types::Decode::decode(buf)?;
                var.0
            };
        }
    } else if attr.optional {
        quote! {
            let #field_name = {
                let present: bool = basalt_types::Decode::decode(buf)?;
                if present {
                    Some(basalt_types::Decode::decode(buf)?)
                } else {
                    None
                }
            };
        }
    } else if attr.length_varint && attr.element_varint {
        quote! {
            let #field_name = {
                let len: basalt_types::VarInt = basalt_types::Decode::decode(buf)?;
                let len = len.0 as usize;
                // Cap allocation to remaining buffer to prevent OOM
                let mut items = Vec::with_capacity(len.min(buf.len()));
                for _ in 0..len {
                    let var: basalt_types::VarInt = basalt_types::Decode::decode(buf)?;
                    items.push(var.0);
                }
                items
            };
        }
    } else if attr.length_varint {
        quote! {
            let #field_name = {
                let len: basalt_types::VarInt = basalt_types::Decode::decode(buf)?;
                let len = len.0 as usize;
                // Cap allocation to remaining buffer to prevent OOM
                let mut items = Vec::with_capacity(len.min(buf.len()));
                for _ in 0..len {
                    items.push(basalt_types::Decode::decode(buf)?);
                }
                items
            };
        }
    } else if attr.rest {
        quote! {
            let #field_name = {
                let rest = buf.to_vec();
                *buf = &buf[buf.len()..];
                rest
            };
        }
    } else {
        quote! {
            let #field_name: #field_type = basalt_types::Decode::decode(buf)?;
        }
    }
}

/// Generates the encoded_size expression for a single field.
pub fn field_size(val: &TokenStream, attr: &FieldAttr) -> TokenStream {
    if attr.varint {
        quote! { basalt_types::EncodedSize::encoded_size(&basalt_types::VarInt(*#val)) }
    } else if attr.optional {
        quote! {
            1 + match #val {
                Some(value) => basalt_types::EncodedSize::encoded_size(value),
                None => 0,
            }
        }
    } else if attr.length_varint && attr.element_varint {
        quote! {
            basalt_types::EncodedSize::encoded_size(&basalt_types::VarInt((#val).len() as i32))
            + (#val).iter().map(|item| basalt_types::EncodedSize::encoded_size(&basalt_types::VarInt(*item))).sum::<usize>()
        }
    } else if attr.length_varint {
        quote! {
            basalt_types::EncodedSize::encoded_size(&basalt_types::VarInt((#val).len() as i32))
            + (#val).iter().map(|item| basalt_types::EncodedSize::encoded_size(item)).sum::<usize>()
        }
    } else if attr.rest {
        quote! { (#val).len() }
    } else {
        quote! { basalt_types::EncodedSize::encoded_size(#val) }
    }
}
