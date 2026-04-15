use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DataEnum, DataStruct, DeriveInput, Fields, Result};

use crate::attrs::{parse_field_attr, parse_variant_attr};

/// Generates the `Encode` implementation for a struct or enum.
///
/// Used by the `#[derive(Encode)]` macro for non-packet types (inner data
/// structures, enums, etc.). For packet structs, use `#[packet(id = N)]`
/// which calls `derive_encode_fields_only` instead.
pub fn derive_encode(input: &DeriveInput) -> Result<TokenStream> {
    match &input.data {
        Data::Struct(data) => derive_encode_struct(input, data),
        Data::Enum(data) => derive_encode_enum(input, data),
        Data::Union(_) => Err(syn::Error::new_spanned(
            input,
            "Encode cannot be derived for unions",
        )),
    }
}

/// Generates the `Encode` implementation encoding only the struct's fields.
///
/// Called by the `#[packet]` attribute macro. Does not generate any packet
/// ID handling — the ID is managed by the framing layer and registry.
pub fn derive_encode_fields_only(input: &DeriveInput) -> Result<TokenStream> {
    match &input.data {
        Data::Struct(data) => derive_encode_struct(input, data),
        Data::Enum(_) => Err(syn::Error::new_spanned(
            input,
            "#[packet] cannot be used on enums",
        )),
        Data::Union(_) => Err(syn::Error::new_spanned(
            input,
            "#[packet] cannot be used on unions",
        )),
    }
}

/// Generates `Encode` for a struct: encodes each field in declaration order.
fn derive_encode_struct(input: &DeriveInput, data: &DataStruct) -> Result<TokenStream> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &data.fields {
        Fields::Named(fields) => &fields.named,
        Fields::Unnamed(_) => {
            return Err(syn::Error::new_spanned(
                input,
                "Encode derive does not support tuple structs",
            ));
        }
        Fields::Unit => {
            return Ok(quote! {
                impl #impl_generics basalt_types::Encode for #name #ty_generics #where_clause {
                    fn encode(&self, buf: &mut Vec<u8>) -> basalt_types::Result<()> {
                        Ok(())
                    }
                }
            });
        }
    };

    let mut field_encodes = Vec::new();
    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        let attr = parse_field_attr(&field.attrs)?;

        let encode = if attr.varint {
            quote! {
                basalt_types::Encode::encode(&basalt_types::VarInt(self.#field_name), buf)?;
            }
        } else if attr.optional {
            quote! {
                match &self.#field_name {
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
                basalt_types::Encode::encode(&basalt_types::VarInt(self.#field_name.len() as i32), buf)?;
                for item in &self.#field_name {
                    basalt_types::Encode::encode(&basalt_types::VarInt(*item), buf)?;
                }
            }
        } else if attr.length_varint {
            quote! {
                basalt_types::Encode::encode(&basalt_types::VarInt(self.#field_name.len() as i32), buf)?;
                for item in &self.#field_name {
                    basalt_types::Encode::encode(item, buf)?;
                }
            }
        } else if attr.rest {
            quote! {
                buf.extend_from_slice(&self.#field_name);
            }
        } else {
            quote! {
                basalt_types::Encode::encode(&self.#field_name, buf)?;
            }
        };

        field_encodes.push(encode);
    }

    Ok(quote! {
        impl #impl_generics basalt_types::Encode for #name #ty_generics #where_clause {
            fn encode(&self, buf: &mut Vec<u8>) -> basalt_types::Result<()> {
                #(#field_encodes)*
                Ok(())
            }
        }
    })
}

/// Generates `Encode` for an enum: encodes a VarInt discriminant followed
/// by the variant's fields (if any).
fn derive_encode_enum(input: &DeriveInput, data: &DataEnum) -> Result<TokenStream> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let mut match_arms = Vec::new();
    let mut next_id: i32 = 0;

    for variant in &data.variants {
        let variant_name = &variant.ident;
        let variant_attr = parse_variant_attr(&variant.attrs)?;
        let id = variant_attr.map_or(next_id, |a| a.id);
        next_id = id + 1;

        let arm = match &variant.fields {
            Fields::Unit => {
                quote! {
                    #name::#variant_name => {
                        basalt_types::Encode::encode(&basalt_types::VarInt(#id), buf)?;
                    }
                }
            }
            Fields::Named(fields) => {
                let field_names: Vec<_> = fields
                    .named
                    .iter()
                    .map(|f| f.ident.as_ref().unwrap())
                    .collect();
                let field_encodes = fields
                    .named
                    .iter()
                    .map(|f| {
                        let fname = f.ident.as_ref().unwrap();
                        let attr = parse_field_attr(&f.attrs)?;
                        Ok(if attr.varint {
                            quote! { basalt_types::Encode::encode(&basalt_types::VarInt(*#fname), buf)?; }
                        } else if attr.optional {
                            quote! {
                                match #fname {
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
                                basalt_types::Encode::encode(&basalt_types::VarInt(#fname.len() as i32), buf)?;
                                for item in #fname {
                                    basalt_types::Encode::encode(&basalt_types::VarInt(*item), buf)?;
                                }
                            }
                        } else if attr.length_varint {
                            quote! {
                                basalt_types::Encode::encode(&basalt_types::VarInt(#fname.len() as i32), buf)?;
                                for item in #fname {
                                    basalt_types::Encode::encode(item, buf)?;
                                }
                            }
                        } else if attr.rest {
                            quote! { buf.extend_from_slice(#fname); }
                        } else {
                            quote! { basalt_types::Encode::encode(#fname, buf)?; }
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                quote! {
                    #name::#variant_name { #(#field_names),* } => {
                        basalt_types::Encode::encode(&basalt_types::VarInt(#id), buf)?;
                        #(#field_encodes)*
                    }
                }
            }
            Fields::Unnamed(fields) => {
                let field_names: Vec<_> = (0..fields.unnamed.len())
                    .map(|i| syn::Ident::new(&format!("f{i}"), proc_macro2::Span::call_site()))
                    .collect();
                let field_encodes: Vec<_> = field_names
                    .iter()
                    .map(|name| {
                        quote! {
                            basalt_types::Encode::encode(#name, buf)?;
                        }
                    })
                    .collect();
                quote! {
                    #name::#variant_name(#(#field_names),*) => {
                        basalt_types::Encode::encode(&basalt_types::VarInt(#id), buf)?;
                        #(#field_encodes)*
                    }
                }
            }
        };
        match_arms.push(arm);
    }

    Ok(quote! {
        impl #impl_generics basalt_types::Encode for #name #ty_generics #where_clause {
            fn encode(&self, buf: &mut Vec<u8>) -> basalt_types::Result<()> {
                match self {
                    #(#match_arms)*
                }
                Ok(())
            }
        }
    })
}
