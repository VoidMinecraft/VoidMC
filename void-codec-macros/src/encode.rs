use crate::attrs::{parse_field_attrs, parse_repr_type, parse_type_attrs, parse_variant_attrs};
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
                                void_codec::VarI32(self.#field_name).encode(buf);
                            }
                        } else if field_attrs.varint64 {
                            quote! {
                                void_codec::VarI64(self.#field_name).encode(buf);
                            }
                        } else if field_attrs.json {
                            quote! {
                                let json_str = serde_json::to_string(&self.#field_name).unwrap();
                                json_str.encode(buf);
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
                    impl void_codec::Encode for #name {
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
                    impl void_codec::Encode for #name {
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
                            Some(id) => id as u32,
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
                                        (#packet_id as u8).encode(buf);
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

                let expanded = quote! {
                    impl void_codec::Encode for #name {
                        fn encode(&self, buf: &mut Vec<u8>) {
                            match self {
                                #(#variants)*
                            }
                        }
                    }
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
                                        void_codec::VarI32(#discriminant as i32).encode(buf);
                                    }
                                } else if type_attrs.varint64 {
                                    quote! {
                                        void_codec::VarI64(#discriminant as i64).encode(buf);
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

                let expanded = quote! {
                    impl void_codec::Encode for #name {
                        fn encode(&self, buf: &mut Vec<u8>) {
                            match self {
                                #(#variants)*
                            }
                        }
                    }
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
