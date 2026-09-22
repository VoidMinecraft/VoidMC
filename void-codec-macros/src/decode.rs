use crate::attrs::{
    checked_len_expr, is_vec_u8, parse_field_attrs, parse_repr_type, parse_type_attrs,
    parse_variant_attrs,
};
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
                                let #field_name = decoder.decode::<voidmc_codec::VarI32>()?.0;
                            }
                        } else if field_attrs.varint64 {
                            quote! {
                                let #field_name = decoder.decode::<voidmc_codec::VarI64>()?.0;
                            }
                        } else if field_attrs.json {
                            quote! {
                                let json_str = decoder.decode::<String>()?;
                                let #field_name = serde_json::from_str(&json_str).map_err(|_| voidmc_codec::DecodeError::InvalidLength)?;
                            }
                        } else if let Some(len_expr) = &field_attrs.fixed_length {
                            let checked_expr = checked_len_expr(len_expr);
                            // Use optimized path for Vec<u8>
                            if is_vec_u8(f) {
                                quote! {
                                    let #field_name = {
                                        let expected_len = (#checked_expr)?;
                                        voidmc_codec::decode_fixed_length_vec_u8(expected_len, decoder)?
                                    };
                                }
                            } else {
                                quote! {
                                    let #field_name = {
                                        let expected_len = (#checked_expr)?;
                                        voidmc_codec::decode_fixed_length_vec(expected_len, decoder)?
                                    };
                                }
                            }
                        } else if field_attrs.remaining {
                            // Remaining attribute: consume all remaining bytes on decode
                            if is_vec_u8(f) {
                                quote! {
                                    let #field_name = voidmc_codec::decode_remaining_vec_u8(decoder)?;
                                }
                            } else {
                                // Error: remaining only works with Vec<u8>
                                return Err(Error::new_spanned(
                                    f,
                                    "remaining attribute only works with Vec<u8>",
                                ));
                            }
                        } else {
                            quote! {
                                let #field_name = decoder.decode::<_>()?;
                            }
                        };

                        Ok(decode_expr)
                    })
                    .collect::<Result<Vec<_>>>()?;

                let field_names = fields.named.iter().map(|f| &f.ident);

                let expanded = quote! {
                    impl voidmc_codec::Decode for #name {
                        fn decode_with(decoder: &mut voidmc_codec::Decoder<'_>) -> Result<Self, voidmc_codec::DecodeError> {
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
                    impl voidmc_codec::Decode for #name {
                        fn decode_with(_decoder: &mut voidmc_codec::Decoder<'_>) -> Result<Self, voidmc_codec::DecodeError> {
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
                                        let inner = decoder.decode::<_>()?;
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
                    impl voidmc_codec::Decode for #name {
                        fn decode_with(decoder: &mut voidmc_codec::Decoder<'_>) -> Result<Self, voidmc_codec::DecodeError> {
                            let packet_id = decoder.decode::<voidmc_codec::VarI32>()?.0;
                            Ok(match packet_id {
                                #(#decode_variants),*
                                _ => return Err(voidmc_codec::DecodeError::InvalidPacketId(u8::try_from(packet_id).ok())),
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
                    quote! { decoder.decode::<voidmc_codec::VarI32>()?.0 as #repr_type_ident }
                } else if type_attrs.varint64 {
                    quote! { decoder.decode::<voidmc_codec::VarI64>()?.0 as #repr_type_ident }
                } else {
                    quote! { decoder.decode::<#repr_type_ident>()? }
                };

                let expanded = quote! {
                    impl voidmc_codec::Decode for #name {
                        fn decode_with(decoder: &mut voidmc_codec::Decoder<'_>) -> Result<Self, voidmc_codec::DecodeError> {
                            let discriminant = #encode_part;
                            Ok(match discriminant {
                                #(#decode_variants)*
                                _ => return Err(voidmc_codec::DecodeError::InvalidPacketId(None)),
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
