use crate::attrs::{
    is_vec_u8, parse_field_attrs, parse_repr_type, parse_type_attrs, parse_variant_attrs,
    transform_expr_for_self,
};
use quote::quote;
use syn::{Data, DeriveInput, Error, Fields, Result};

pub fn derive_encode(input: &DeriveInput) -> Result<proc_macro2::TokenStream> {
    let name = &input.ident;
    let type_attrs = parse_type_attrs(&input.attrs)?;

    match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => {
                let encode_fields = fields
                    .named
                    .iter()
                    .map(|f| {
                        let field_name = &f.ident;
                        let field_attrs = parse_field_attrs(&f.attrs)?;

                        let encode_expr = if field_attrs.varint32 {
                            quote! {
                                voidmc_codec::VarI32(self.#field_name).encode(buf);
                            }
                        } else if field_attrs.varint64 {
                            quote! {
                                voidmc_codec::VarI64(self.#field_name).encode(buf);
                            }
                        } else if field_attrs.json {
                            quote! {
                                let json_str = serde_json::to_string(&self.#field_name).unwrap();
                                json_str.encode(buf);
                            }
                        } else if let Some(len_expr) = &field_attrs.fixed_length {
                            let transformed_expr = transform_expr_for_self(len_expr);
                            // Use optimized path for Vec<u8>
                            if is_vec_u8(f) {
                                quote! {
                                    {
                                        let expected_len = ((#transformed_expr) as i64) as usize;
                                        match voidmc_codec::encode_fixed_length_vec_u8(&self.#field_name, expected_len, buf) {
                                            Ok(_) => {},
                                            Err(e) => panic!("{}", e),
                                        }
                                    }
                                }
                            } else {
                                quote! {
                                    {
                                        let expected_len = ((#transformed_expr) as i64) as usize;
                                        match voidmc_codec::encode_fixed_length_vec(&self.#field_name, expected_len, buf) {
                                            Ok(_) => {},
                                            Err(e) => panic!("{}", e),
                                        }
                                    }
                                }
                            }
                        } else if field_attrs.remaining {
                            // Remaining attribute: consume all remaining bytes on decode
                            if is_vec_u8(f) {
                                quote! {
                                    voidmc_codec::encode_remaining_vec_u8(&self.#field_name, buf);
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
                                self.#field_name.encode(buf);
                            }
                        };

                        Ok(encode_expr)
                    })
                    .collect::<Result<Vec<_>>>()?;

                let expanded = quote! {
                    impl voidmc_codec::Encode for #name {
                        fn encode(&self, buf: &mut Vec<u8>) {
                            #(#encode_fields)*
                        }
                    }
                };

                Ok(expanded)
            }
            Fields::Unnamed(_) => Err(Error::new_spanned(
                input,
                "Encode derive for tuple structs is not supported",
            )),
            Fields::Unit => {
                let expanded = quote! {
                    impl voidmc_codec::Encode for #name {
                        fn encode(&self, _buf: &mut Vec<u8>) {}
                    }
                };
                Ok(expanded)
            }
        },
        Data::Enum(data) => {
            let repr_type = parse_repr_type(&input.attrs)?;

            if type_attrs.tagged {
                // Tagged enum with explicit packet IDs
                let variants = data
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
                                    Self::#variant_name(inner) => {
                                        voidmc_codec::VarI32(#packet_id).encode(buf);
                                        inner.encode(buf);
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

                let from_impls = tagged_from_impls(name, data, type_attrs.wrap.as_ref());

                let expanded = quote! {
                    impl voidmc_codec::Encode for #name {
                        fn encode(&self, buf: &mut Vec<u8>) {
                            match self {
                                #(#variants)*
                            }
                        }
                    }

                    #from_impls
                };

                Ok(expanded)
            } else if let Some(repr) = repr_type {
                // Repr enum with explicit discriminants
                let repr_type_ident = syn::Ident::new(&repr, proc_macro2::Span::call_site());

                let variants = data
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

                                let encode_expr = if type_attrs.varint32 {
                                    quote! {
                                        voidmc_codec::VarI32(#discriminant as i32).encode(buf);
                                    }
                                } else if type_attrs.varint64 {
                                    quote! {
                                        voidmc_codec::VarI64(#discriminant as i64).encode(buf);
                                    }
                                } else {
                                    quote! {
                                        (#discriminant as #repr_type_ident).encode(buf);
                                    }
                                };

                                Ok(quote! {
                                    Self::#variant_name => {
                                        #encode_expr
                                    }
                                })
                            }
                            _ => Err(Error::new_spanned(
                                v,
                                "repr enum variant must be a unit variant",
                            )),
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;

                let from_impls = tagged_from_impls(name, data, type_attrs.wrap.as_ref());

                let expanded = quote! {
                    impl voidmc_codec::Encode for #name {
                        fn encode(&self, buf: &mut Vec<u8>) {
                            match self {
                                #(#variants)*
                            }
                        }
                    }

                    #from_impls
                };

                Ok(expanded)
            } else {
                Err(Error::new_spanned(
                    input,
                    "Enum Encode derive requires #[codec(tagged)] attribute or #[repr(...)] attribute",
                ))
            }
        }
        Data::Union(_) => Err(Error::new_spanned(input, "Unions are not supported")),
    }
}

fn tagged_from_impls(
    name: &syn::Ident,
    data: &syn::DataEnum,
    wrap: Option<&syn::Path>,
) -> proc_macro2::TokenStream {
    let wrap_type = wrap.map(|path| {
        let mut type_path = path.clone();
        type_path.segments.pop();
        type_path.segments.pop_punct();
        type_path
    });

    let per_variant = data.variants.iter().filter_map(|v| {
        let variant = &v.ident;
        let inner = match &v.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => &fields.unnamed[0].ty,
            _ => return None,
        };
        let wrapped = wrap.zip(wrap_type.as_ref()).map(|(path, ty)| {
            quote! {
                impl From<#inner> for #ty {
                    fn from(packet: #inner) -> Self {
                        #path(#name::#variant(packet))
                    }
                }
            }
        });
        Some(quote! {
            impl From<#inner> for #name {
                fn from(packet: #inner) -> Self {
                    #name::#variant(packet)
                }
            }
            #wrapped
        })
    });

    let enum_level = wrap.zip(wrap_type.as_ref()).map(|(path, ty)| {
        quote! {
            impl From<#name> for #ty {
                fn from(packet: #name) -> Self {
                    #path(packet)
                }
            }
        }
    });

    quote! {
        #(#per_variant)*
        #enum_level
    }
}
