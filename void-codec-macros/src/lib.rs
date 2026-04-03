use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod attrs;
mod decode;
mod encode;

#[proc_macro_derive(Encode, attributes(codec))]
pub fn derive_encode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    encode::derive_encode(&input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

#[proc_macro_derive(Decode, attributes(codec))]
pub fn derive_decode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    decode::derive_decode(&input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}
