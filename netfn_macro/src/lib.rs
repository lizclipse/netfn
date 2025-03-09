#![warn(clippy::pedantic)]

use netfn_codegen::service_generate;
use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn service(args: TokenStream, input: TokenStream) -> TokenStream {
    service_generate(args.into(), input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
