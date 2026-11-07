use quote::quote;
use syn::{Data, DeriveInput, Error, Fields, Result};

pub fn derive_packet_name(input: &DeriveInput) -> Result<proc_macro2::TokenStream> {
    let name = &input.ident;
    let Data::Enum(data) = &input.data else {
        return Err(Error::new_spanned(
            input,
            "PacketName derives on enums only",
        ));
    };
    let arms = data
        .variants
        .iter()
        .map(|variant| {
            let ident = &variant.ident;
            let label = ident.to_string();
            match &variant.fields {
                Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                    Ok(quote! { Self::#ident(_) => #label, })
                }
                _ => Err(Error::new_spanned(
                    variant,
                    "PacketName variants hold exactly one unnamed field",
                )),
            }
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(quote! {
        impl voidmc_codec::PacketName for #name {
            fn packet_name(&self) -> &'static str {
                match self {
                    #(#arms)*
                }
            }
        }
    })
}
