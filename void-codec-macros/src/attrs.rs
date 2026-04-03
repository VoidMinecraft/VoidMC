use syn::{Attribute, Expr, Field, GenericArgument, LitInt, PathArguments, Result, Type};

/// Transform an expression to prefix bare identifiers with `self.` for use in impl blocks
/// E.g., `length * factor` becomes `self.length * self.factor`
pub fn transform_expr_for_self(expr: &Expr) -> Expr {
    match expr {
        Expr::Path(expr_path)
            if expr_path.path.segments.len() == 1 && expr_path.qself.is_none() =>
        {
            // Simple identifier - wrap with self.
            let ident = &expr_path.path.segments[0].ident;
            syn::parse_quote!(self.#ident)
        }
        Expr::Binary(expr_bin) => {
            // Recursively transform both sides
            let left = transform_expr_for_self(&expr_bin.left);
            let right = transform_expr_for_self(&expr_bin.right);
            let op = &expr_bin.op;
            syn::parse_quote!(#left #op #right)
        }
        Expr::Paren(expr_paren) => {
            let inner = transform_expr_for_self(&expr_paren.expr);
            syn::parse_quote!((#inner))
        }
        // For other expressions, return as-is
        _ => expr.clone(),
    }
}

/// Same as above but without self. prefix for use in function context where vars are local
pub fn transform_expr_for_local(expr: &Expr) -> Expr {
    // Just return as-is since identifiers are already local variables in scope
    expr.clone()
}

/// Check if a field type is `Vec<u8>`
pub fn is_vec_u8(field: &Field) -> bool {
    match &field.ty {
        Type::Path(type_path) => {
            // Check if the path is "Vec"
            if type_path.path.segments.len() != 1 {
                return false;
            }
            let segment = &type_path.path.segments[0];
            if segment.ident != "Vec" {
                return false;
            }

            // Check the generic argument is u8
            match &segment.arguments {
                PathArguments::AngleBracketed(args) => {
                    args.args.len() == 1
                        && args.args.iter().any(|arg| {
                            if let GenericArgument::Type(Type::Path(type_path)) = arg {
                                type_path.path.segments.len() == 1
                                    && type_path.path.segments[0].ident == "u8"
                            } else {
                                false
                            }
                        })
                }
                _ => false,
            }
        }
        _ => false,
    }
}

pub struct FieldAttrs {
    pub varint32: bool,
    pub varint64: bool,
    pub json: bool,
    pub fixed_length: Option<Box<Expr>>,
    pub remaining: bool,
}

impl Default for FieldAttrs {
    fn default() -> Self {
        Self {
            varint32: false,
            varint64: false,
            json: false,
            fixed_length: None,
            remaining: false,
        }
    }
}

pub struct TypeAttrs {
    pub tagged: bool,
    pub varint32: bool,
    pub varint64: bool,
}

impl Default for TypeAttrs {
    fn default() -> Self {
        Self {
            tagged: false,
            varint32: false,
            varint64: false,
        }
    }
}

pub struct VariantAttrs {
    pub packet_id: Option<u8>,
}

impl Default for VariantAttrs {
    fn default() -> Self {
        Self { packet_id: None }
    }
}

pub fn parse_field_attrs(attrs: &[Attribute]) -> Result<FieldAttrs> {
    let mut field_attrs = FieldAttrs::default();

    for attr in attrs {
        if !attr.path().is_ident("codec") {
            continue;
        }

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("varint32") {
                field_attrs.varint32 = true;
                Ok(())
            } else if meta.path.is_ident("varint64") {
                field_attrs.varint64 = true;
                Ok(())
            } else if meta.path.is_ident("json") {
                field_attrs.json = true;
                Ok(())
            } else if meta.path.is_ident("fixed_length") {
                let _eq: syn::token::Eq = meta.input.parse()?;
                let expr: Expr = meta.input.parse()?;
                field_attrs.fixed_length = Some(Box::new(expr));
                Ok(())
            } else if meta.path.is_ident("remaining") {
                field_attrs.remaining = true;
                Ok(())
            } else {
                Err(meta.error("unknown field codec attribute"))
            }
        })?;
    }

    Ok(field_attrs)
}

pub fn parse_type_attrs(attrs: &[Attribute]) -> Result<TypeAttrs> {
    let mut type_attrs = TypeAttrs::default();

    for attr in attrs {
        if !attr.path().is_ident("codec") {
            continue;
        }

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("tagged") {
                type_attrs.tagged = true;
                Ok(())
            } else if meta.path.is_ident("varint32") {
                type_attrs.varint32 = true;
                Ok(())
            } else if meta.path.is_ident("varint64") {
                type_attrs.varint64 = true;
                Ok(())
            } else {
                Err(meta.error("unknown type codec attribute"))
            }
        })?;
    }

    Ok(type_attrs)
}

pub fn parse_variant_attrs(attrs: &[Attribute]) -> Result<VariantAttrs> {
    let mut variant_attrs = VariantAttrs::default();

    for attr in attrs {
        if !attr.path().is_ident("codec") {
            continue;
        }

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("packet_id") {
                let _eq: syn::token::Eq = meta.input.parse()?;
                let lit: LitInt = meta.input.parse()?;
                variant_attrs.packet_id = Some(lit.base10_parse()?);
                Ok(())
            } else {
                Err(meta.error("unknown variant codec attribute"))
            }
        })?;
    }

    Ok(variant_attrs)
}

pub fn parse_repr_type(attrs: &[Attribute]) -> Result<Option<String>> {
    for attr in attrs {
        if !attr.path().is_ident("repr") {
            continue;
        }

        let mut repr_type = None;
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("u8") {
                repr_type = Some("u8".to_string());
                Ok(())
            } else if meta.path.is_ident("i32") {
                repr_type = Some("i32".to_string());
                Ok(())
            } else if meta.path.is_ident("u32") {
                repr_type = Some("u32".to_string());
                Ok(())
            } else if meta.path.is_ident("i64") {
                repr_type = Some("i64".to_string());
                Ok(())
            } else if meta.path.is_ident("u64") {
                repr_type = Some("u64".to_string());
                Ok(())
            } else if meta.path.is_ident("i16") {
                repr_type = Some("i16".to_string());
                Ok(())
            } else if meta.path.is_ident("u16") {
                repr_type = Some("u16".to_string());
                Ok(())
            } else {
                Err(meta.error("unsupported repr type for codec"))
            }
        })?;
        return Ok(repr_type);
    }

    Ok(None)
}
