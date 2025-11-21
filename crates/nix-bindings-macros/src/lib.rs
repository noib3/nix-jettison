//! TODO: docs.

mod args;
mod attrset;
mod entry;
mod primop;
use proc_macro::TokenStream;
use syn::parse_macro_input;

/// TODO: docs
#[proc_macro_derive(Args)]
pub fn args(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);
    args::args(input).unwrap_or_else(syn::Error::into_compile_error).into()
}

/// Marks the entrypoint function of a Nix plugin.
#[proc_macro_attribute]
pub fn entry(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as syn::ItemFn);
    entry::entry(attr, item)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// TODO: docs
#[proc_macro_derive(PrimOp)]
pub fn primop(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);
    primop::primop(input).unwrap_or_else(syn::Error::into_compile_error).into()
}

/// TODO: docs
#[proc_macro]
pub fn attrset(input: TokenStream) -> TokenStream {
    attrset::attrset(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
