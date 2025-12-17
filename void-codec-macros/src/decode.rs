use crate::attrs::{parse_field_attrs, parse_repr_type, parse_type_attrs, parse_variant_attrs};
use quote::quote;
use syn::{Data, DeriveInput, Error, Fields, Result};

pub fn derive_decode(input: &DeriveInput) -> Result<proc_macro2::TokenStream> {
    let name = &input.ident;
    let type_attrs = parse_type_attrs(&input.attrs)?;

    match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => {
                let decode_fields = fields
                    .named
                    .iter()
                    .map(|f| {
                        let field_name = &f.ident;
                        let field_attrs = parse_field_attrs(&f.attrs)?;

                        let decode_expr = if field_attrs.varint32 {
                            quote! {
                                let #field_name = void_codec::VarI32::decode(buf)?.0;
                            }
                        } else if field_attrs.varint64 {
                            quote! {
                                let #field_name = void_codec::VarI64::decode(buf)?.0;
                            }
                        } else if field_attrs.json {
                            quote! {
                                let json_str = String::decode(buf)?;
                                let #field_name = serde_json::from_str(&json_str).map_err(|_| void_codec::DecodeError::InvalidLength)?;
                            }
                        } else {
                            quote! {
                                let #field_name = <_>::decode(buf)?;
                            }
                        };

                        Ok(decode_expr)
                    })
                    .collect::<Result<Vec<_>>>()?;

                let field_names = fields.named.iter().map(|f| &f.ident);

                let expanded = quote! {
                    impl void_codec::Decode for #name {
                        fn decode(buf: &mut &[u8]) -> Result<Self, void_codec::DecodeError> {
                            #(#decode_fields)*
                            Ok(Self {
                                #(#field_names),*
                            })
                        }
                    }
                };

                Ok(expanded)
            }
            Fields::Unnamed(_) => Err(Error::new_spanned(
                input,
                "Decode derive for tuple structs is not supported",
            )),
            Fields::Unit => {
                let expanded = quote! {
                    impl void_codec::Decode for #name {
                        fn decode(_buf: &mut &[u8]) -> Result<Self, void_codec::DecodeError> {
                            Ok(Self)
                        }
                    }
                };
                Ok(expanded)
            }
        },
        Data::Enum(data) => {
            let repr_type = parse_repr_type(&input.attrs)?;

            if type_attrs.tagged {
                // Tagged enum with explicit packet IDs
                let decode_variants = data
                    .variants
                    .iter()
                    .map(|v| {
                        let variant_name = &v.ident;
                        let variant_attrs = parse_variant_attrs(&v.attrs)?;

                        let packet_id = match variant_attrs.packet_id {
                            Some(id) => id,
                            None => {
                                return Err(Error::new_spanned(
                                v,
                                "tagged enum variant must have #[codec(packet_id = ...)] attribute",
                            ));
                            }
                        };

                        Ok(match &v.fields {
                            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                                quote! {
                                    #packet_id => {
                                        let inner = <_>::decode(buf)?;
                                        Self::#variant_name(inner)
                                    }
                                }
                            }
                            _ => {
                                return Err(Error::new_spanned(
                                    v,
                                    "Enum variant must have exactly one unnamed field",
                                ));
                            }
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;

                let expanded = quote! {
                    impl void_codec::Decode for #name {
                        fn decode(buf: &mut &[u8]) -> Result<Self, void_codec::DecodeError> {
                            let packet_id = u8::decode(buf)?;
                            Ok(match packet_id {
                                #(#decode_variants),*
                                _ => return Err(void_codec::DecodeError::InvalidPacketId),
                            })
                        }
                    }
                };

                Ok(expanded)
            } else if let Some(repr) = repr_type {
                // Repr enum with explicit discriminants
                let repr_type_ident = syn::Ident::new(&repr, proc_macro2::Span::call_site());

                let decode_variants = data
                    .variants
                    .iter()
                    .map(|v| {
                        let variant_name = &v.ident;

                        match &v.fields {
                            Fields::Unit => {
                                let discriminant = v
                                    .discriminant
                                    .as_ref()
                                    .map(|(_, expr)| {
                                        quote! { #expr }
                                    })
                                    .ok_or_else(|| {
                                        Error::new_spanned(
                                            v,
                                            "repr enum variant must have explicit discriminant",
                                        )
                                    })?;

                                Ok(quote! {
                                    #discriminant => Self::#variant_name,
                                })
                            }
                            _ => Err(Error::new_spanned(
                                v,
                                "repr enum variant must be a unit variant",
                            )),
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;

                let encode_part = if type_attrs.varint32 {
                    quote! { void_codec::VarI32::decode(buf)?.0 as #repr_type_ident }
                } else if type_attrs.varint64 {
                    quote! { void_codec::VarI64::decode(buf)?.0 as #repr_type_ident }
                } else {
                    quote! { #repr_type_ident::decode(buf)? }
                };

                let expanded = quote! {
                    impl void_codec::Decode for #name {
                        fn decode(buf: &mut &[u8]) -> Result<Self, void_codec::DecodeError> {
                            let discriminant = #encode_part;
                            Ok(match discriminant {
                                #(#decode_variants)*
                                _ => return Err(void_codec::DecodeError::InvalidPacketId),
                            })
                        }
                    }
                };

                Ok(expanded)
            } else {
                Err(Error::new_spanned(
                    input,
                    "Enum Decode derive requires #[codec(tagged)] attribute or #[repr(...)] attribute",
                ))
            }
        }
        Data::Union(_) => Err(Error::new_spanned(input, "Unions are not supported")),
    }
}
