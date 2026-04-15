use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DataEnum, DataStruct, DeriveInput, Fields, Result};

use crate::attrs::{parse_field_attr, parse_variant_attr};

/// Generates the `EncodedSize` implementation for a struct or enum.
pub fn derive_encoded_size(input: &DeriveInput) -> Result<TokenStream> {
    match &input.data {
        Data::Struct(data) => derive_encoded_size_struct(input, data),
        Data::Enum(data) => derive_encoded_size_enum(input, data),
        Data::Union(_) => Err(syn::Error::new_spanned(
            input,
            "EncodedSize cannot be derived for unions",
        )),
    }
}

/// Generates `EncodedSize` encoding only the struct's fields.
///
/// Called by the `#[packet]` attribute macro.
pub fn derive_encoded_size_fields_only(input: &DeriveInput) -> Result<TokenStream> {
    match &input.data {
        Data::Struct(data) => derive_encoded_size_struct(input, data),
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

/// Generates `EncodedSize` for a struct: sums the encoded size of each field.
fn derive_encoded_size_struct(input: &DeriveInput, data: &DataStruct) -> Result<TokenStream> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &data.fields {
        Fields::Named(fields) => &fields.named,
        Fields::Unnamed(_) => {
            return Err(syn::Error::new_spanned(
                input,
                "EncodedSize derive does not support tuple structs",
            ));
        }
        Fields::Unit => {
            return Ok(quote! {
                impl #impl_generics basalt_types::EncodedSize for #name #ty_generics #where_clause {
                    fn encoded_size(&self) -> usize {
                        0
                    }
                }
            });
        }
    };

    let mut field_sizes = Vec::new();
    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        let attr = parse_field_attr(&field.attrs)?;

        let size = if attr.varint {
            quote! {
                basalt_types::EncodedSize::encoded_size(&basalt_types::VarInt(self.#field_name))
            }
        } else if attr.optional {
            quote! {
                1 + match &self.#field_name {
                    Some(value) => basalt_types::EncodedSize::encoded_size(value),
                    None => 0,
                }
            }
        } else if attr.length_varint && attr.element_varint {
            quote! {
                basalt_types::EncodedSize::encoded_size(&basalt_types::VarInt(self.#field_name.len() as i32))
                + self.#field_name.iter().map(|item| basalt_types::EncodedSize::encoded_size(&basalt_types::VarInt(*item))).sum::<usize>()
            }
        } else if attr.length_varint {
            quote! {
                basalt_types::EncodedSize::encoded_size(&basalt_types::VarInt(self.#field_name.len() as i32))
                + self.#field_name.iter().map(|item| basalt_types::EncodedSize::encoded_size(item)).sum::<usize>()
            }
        } else if attr.rest {
            quote! {
                self.#field_name.len()
            }
        } else {
            quote! {
                basalt_types::EncodedSize::encoded_size(&self.#field_name)
            }
        };

        field_sizes.push(size);
    }

    Ok(quote! {
        impl #impl_generics basalt_types::EncodedSize for #name #ty_generics #where_clause {
            fn encoded_size(&self) -> usize {
                #(#field_sizes)+*
            }
        }
    })
}

/// Generates `EncodedSize` for an enum: VarInt discriminant size plus
/// the variant's fields size.
fn derive_encoded_size_enum(input: &DeriveInput, data: &DataEnum) -> Result<TokenStream> {
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
                        basalt_types::EncodedSize::encoded_size(&basalt_types::VarInt(#id))
                    }
                }
            }
            Fields::Named(fields) => {
                let field_names: Vec<_> = fields
                    .named
                    .iter()
                    .map(|f| f.ident.as_ref().unwrap())
                    .collect();
                let field_sizes = fields
                    .named
                    .iter()
                    .map(|f| {
                        let fname = f.ident.as_ref().unwrap();
                        let attr = parse_field_attr(&f.attrs)?;
                        Ok(if attr.varint {
                            quote! { basalt_types::EncodedSize::encoded_size(&basalt_types::VarInt(*#fname)) }
                        } else if attr.optional {
                            quote! {
                                1 + match #fname {
                                    Some(value) => basalt_types::EncodedSize::encoded_size(value),
                                    None => 0,
                                }
                            }
                        } else if attr.length_varint && attr.element_varint {
                            quote! {
                                basalt_types::EncodedSize::encoded_size(&basalt_types::VarInt(#fname.len() as i32))
                                + #fname.iter().map(|item| basalt_types::EncodedSize::encoded_size(&basalt_types::VarInt(*item))).sum::<usize>()
                            }
                        } else if attr.length_varint {
                            quote! {
                                basalt_types::EncodedSize::encoded_size(&basalt_types::VarInt(#fname.len() as i32))
                                + #fname.iter().map(|item| basalt_types::EncodedSize::encoded_size(item)).sum::<usize>()
                            }
                        } else if attr.rest {
                            quote! { #fname.len() }
                        } else {
                            quote! { basalt_types::EncodedSize::encoded_size(#fname) }
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                quote! {
                    #name::#variant_name { #(#field_names),* } => {
                        basalt_types::EncodedSize::encoded_size(&basalt_types::VarInt(#id))
                        #(+ #field_sizes)*
                    }
                }
            }
            Fields::Unnamed(fields) => {
                let field_names: Vec<_> = (0..fields.unnamed.len())
                    .map(|i| syn::Ident::new(&format!("f{i}"), proc_macro2::Span::call_site()))
                    .collect();
                let field_sizes: Vec<_> = field_names
                    .iter()
                    .map(|name| {
                        quote! {
                            basalt_types::EncodedSize::encoded_size(#name)
                        }
                    })
                    .collect();
                quote! {
                    #name::#variant_name(#(#field_names),*) => {
                        basalt_types::EncodedSize::encoded_size(&basalt_types::VarInt(#id))
                        #(+ #field_sizes)*
                    }
                }
            }
        };
        match_arms.push(arm);
    }

    Ok(quote! {
        impl #impl_generics basalt_types::EncodedSize for #name #ty_generics #where_clause {
            fn encoded_size(&self) -> usize {
                match self {
                    #(#match_arms)*
                }
            }
        }
    })
}
