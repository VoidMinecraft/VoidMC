use syn::{Attribute, LitInt, Result};

pub struct FieldAttrs {
    pub vari32: bool,
    pub varint64: bool,
}

impl Default for FieldAttrs {
    fn default() -> Self {
        Self {
            vari32: false,
            varint64: false,
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
            if meta.path.is_ident("vari32") {
                field_attrs.vari32 = true;
                Ok(())
            } else if meta.path.is_ident("varint64") {
                field_attrs.varint64 = true;
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
