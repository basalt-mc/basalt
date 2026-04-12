use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DataEnum, DataStruct, DeriveInput, Fields, Result};

use crate::attrs::{parse_field_attr, parse_variant_attr};

/// Generates the `Decode` implementation for a struct or enum.
pub fn derive_decode(input: &DeriveInput) -> Result<TokenStream> {
    match &input.data {
        Data::Struct(data) => derive_decode_struct(input, data),
        Data::Enum(data) => derive_decode_enum(input, data),
        Data::Union(_) => Err(syn::Error::new_spanned(
            input,
            "Decode cannot be derived for unions",
        )),
    }
}

/// Generates `Decode` for a struct: decodes each field in declaration order.
///
/// The packet ID is NOT decoded here — it is handled by the framing layer
/// and the packet registry dispatch. The struct only decodes its own fields.
fn derive_decode_struct(input: &DeriveInput, data: &DataStruct) -> Result<TokenStream> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &data.fields {
        Fields::Named(fields) => &fields.named,
        Fields::Unnamed(_) => {
            return Err(syn::Error::new_spanned(
                input,
                "Decode derive does not support tuple structs",
            ));
        }
        Fields::Unit => {
            return Ok(quote! {
                impl #impl_generics basalt_types::Decode for #name #ty_generics #where_clause {
                    fn decode(buf: &mut &[u8]) -> basalt_types::Result<Self> {
                        Ok(#name)
                    }
                }
            });
        }
    };

    let mut field_decodes = Vec::new();
    let mut field_names = Vec::new();

    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        let field_type = &field.ty;
        let attr = parse_field_attr(&field.attrs)?;

        field_names.push(field_name);

        let decode = if attr.varint {
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
                    let mut items = Vec::with_capacity(len);
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
                    let mut items = Vec::with_capacity(len);
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
        };

        field_decodes.push(decode);
    }

    Ok(quote! {
        impl #impl_generics basalt_types::Decode for #name #ty_generics #where_clause {
            fn decode(buf: &mut &[u8]) -> basalt_types::Result<Self> {
                #(#field_decodes)*
                Ok(#name {
                    #(#field_names),*
                })
            }
        }
    })
}

/// Generates `Decode` for an enum: reads a VarInt discriminant and decodes
/// the corresponding variant's fields.
fn derive_decode_enum(input: &DeriveInput, data: &DataEnum) -> Result<TokenStream> {
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
                    #id => Ok(#name::#variant_name),
                }
            }
            Fields::Named(fields) => {
                let field_names: Vec<_> = fields
                    .named
                    .iter()
                    .map(|f| f.ident.as_ref().unwrap())
                    .collect();
                let field_decodes = fields
                    .named
                    .iter()
                    .map(|f| {
                        let fname = f.ident.as_ref().unwrap();
                        let fty = &f.ty;
                        let attr = parse_field_attr(&f.attrs)?;
                        Ok(if attr.varint {
                            quote! {
                                let #fname = {
                                    let var: basalt_types::VarInt = basalt_types::Decode::decode(buf)?;
                                    var.0
                                };
                            }
                        } else if attr.optional {
                            quote! {
                                let #fname = {
                                    let present: bool = basalt_types::Decode::decode(buf)?;
                                    if present {
                                        Some(basalt_types::Decode::decode(buf)?)
                                    } else {
                                        None
                                    }
                                };
                            }
                        } else if attr.length_varint {
                            quote! {
                                let #fname = {
                                    let len: basalt_types::VarInt = basalt_types::Decode::decode(buf)?;
                                    let len = len.0 as usize;
                                    let mut items = Vec::with_capacity(len);
                                    for _ in 0..len {
                                        items.push(basalt_types::Decode::decode(buf)?);
                                    }
                                    items
                                };
                            }
                        } else if attr.rest {
                            quote! {
                                let #fname = {
                                    let rest = buf.to_vec();
                                    *buf = &buf[buf.len()..];
                                    rest
                                };
                            }
                        } else {
                            quote! {
                                let #fname: #fty = basalt_types::Decode::decode(buf)?;
                            }
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                quote! {
                    #id => {
                        #(#field_decodes)*
                        Ok(#name::#variant_name { #(#field_names),* })
                    }
                }
            }
            Fields::Unnamed(fields) => {
                let field_names: Vec<_> = (0..fields.unnamed.len())
                    .map(|i| syn::Ident::new(&format!("f{i}"), proc_macro2::Span::call_site()))
                    .collect();
                let field_types: Vec<_> = fields.unnamed.iter().map(|f| &f.ty).collect();
                let field_decodes: Vec<_> = field_names
                    .iter()
                    .zip(field_types.iter())
                    .map(|(name, ty)| {
                        quote! {
                            let #name: #ty = basalt_types::Decode::decode(buf)?;
                        }
                    })
                    .collect();
                quote! {
                    #id => {
                        #(#field_decodes)*
                        Ok(#name::#variant_name(#(#field_names),*))
                    }
                }
            }
        };
        match_arms.push(arm);
    }

    let name_str = name.to_string();
    Ok(quote! {
        impl #impl_generics basalt_types::Decode for #name #ty_generics #where_clause {
            fn decode(buf: &mut &[u8]) -> basalt_types::Result<Self> {
                let discriminant: basalt_types::VarInt = basalt_types::Decode::decode(buf)?;
                match discriminant.0 {
                    #(#match_arms)*
                    other => Err(basalt_types::Error::InvalidData(
                        format!("unknown {} discriminant: {}", #name_str, other)
                    )),
                }
            }
        }
    })
}
