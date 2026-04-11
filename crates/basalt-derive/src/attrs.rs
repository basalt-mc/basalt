use proc_macro2::Span;
use syn::{Attribute, Expr, ExprLit, ExprUnary, Lit, Result, UnOp};

/// Parsed `#[packet(id = 0x00)]` attribute on a struct.
#[derive(Debug, Clone)]
pub struct PacketAttr {
    /// The packet ID as an integer literal.
    pub id: i32,
}

/// Parsed `#[field(...)]` attribute on a struct field.
#[derive(Debug, Clone, Default)]
pub struct FieldAttr {
    /// Encode this integer as VarInt (i32) or VarLong (i64).
    pub varint: bool,

    /// This field has a VarInt length prefix (for Vec types).
    pub length_varint: bool,

    /// This field is a boolean-prefixed Option (Minecraft pattern).
    pub optional: bool,

    /// This field consumes all remaining bytes (must be the last field).
    pub rest: bool,
}

/// Parsed `#[variant(id = N)]` attribute on an enum variant.
#[derive(Debug, Clone)]
pub struct VariantAttr {
    /// Explicit discriminant value for this variant.
    pub id: i32,
}

/// Parses an integer expression, handling both positive literals (`42`)
/// and negative literals (`-1`) which Rust parses as unary minus + int literal.
fn parse_int_expr(expr: &Expr) -> syn::Result<i32> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Int(lit_int),
            ..
        }) => lit_int.base10_parse::<i32>(),
        Expr::Unary(ExprUnary {
            op: UnOp::Neg(_),
            expr: inner,
            ..
        }) => {
            if let Expr::Lit(ExprLit {
                lit: Lit::Int(lit_int),
                ..
            }) = inner.as_ref()
            {
                Ok(-lit_int.base10_parse::<i32>()?)
            } else {
                Err(syn::Error::new_spanned(expr, "expected integer literal"))
            }
        }
        _ => Err(syn::Error::new_spanned(expr, "expected integer literal")),
    }
}

/// Extracts the `#[packet(id = ...)]` attribute from a list of attributes.
///
/// Returns `None` if no `#[packet]` attribute is present. Returns an error
/// if the attribute is malformed (missing `id`, wrong type, etc.).
pub fn parse_packet_attr(attrs: &[Attribute]) -> Result<Option<PacketAttr>> {
    for attr in attrs {
        if !attr.path().is_ident("packet") {
            continue;
        }

        let mut id = None;
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("id") {
                let value = meta.value()?;
                let expr: Expr = value.parse()?;
                id = Some(parse_int_expr(&expr)?);
                Ok(())
            } else {
                Err(meta.error("expected `id`"))
            }
        })?;

        match id {
            Some(id) => return Ok(Some(PacketAttr { id })),
            None => {
                return Err(syn::Error::new(
                    Span::call_site(),
                    "#[packet] requires `id` parameter",
                ));
            }
        }
    }
    Ok(None)
}

/// Extracts the `#[field(...)]` attribute from a list of attributes.
///
/// Supports multiple flags: `varint`, `optional`, `rest`, `length = "varint"`.
/// Returns a default `FieldAttr` if no `#[field]` attribute is present.
pub fn parse_field_attr(attrs: &[Attribute]) -> Result<FieldAttr> {
    let mut field_attr = FieldAttr::default();

    for attr in attrs {
        if !attr.path().is_ident("field") {
            continue;
        }

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("varint") {
                field_attr.varint = true;
                Ok(())
            } else if meta.path.is_ident("optional") {
                field_attr.optional = true;
                Ok(())
            } else if meta.path.is_ident("rest") {
                field_attr.rest = true;
                Ok(())
            } else if meta.path.is_ident("length") {
                let value = meta.value()?;
                let lit: Lit = value.parse()?;
                if let Lit::Str(s) = lit {
                    if s.value() == "varint" {
                        field_attr.length_varint = true;
                        Ok(())
                    } else {
                        Err(syn::Error::new_spanned(s, "expected \"varint\""))
                    }
                } else {
                    Err(syn::Error::new_spanned(lit, "expected string literal"))
                }
            } else {
                Err(meta.error("expected `varint`, `optional`, `rest`, or `length`"))
            }
        })?;
    }

    Ok(field_attr)
}

/// Extracts the `#[variant(id = N)]` attribute from a list of attributes.
///
/// Returns `None` if no `#[variant]` attribute is present.
pub fn parse_variant_attr(attrs: &[Attribute]) -> Result<Option<VariantAttr>> {
    for attr in attrs {
        if !attr.path().is_ident("variant") {
            continue;
        }

        let mut id = None;
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("id") {
                let value = meta.value()?;
                let expr: Expr = value.parse()?;
                id = Some(parse_int_expr(&expr)?);
                Ok(())
            } else {
                Err(meta.error("expected `id`"))
            }
        })?;

        match id {
            Some(id) => return Ok(Some(VariantAttr { id })),
            None => {
                return Err(syn::Error::new(
                    Span::call_site(),
                    "#[variant] requires `id` parameter",
                ));
            }
        }
    }
    Ok(None)
}
