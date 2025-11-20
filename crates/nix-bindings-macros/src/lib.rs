//! TODO: docs.

mod entry;
use proc_macro::TokenStream;
use syn::parse_macro_input;

/// Marks the entrypoint function of a Nix plugin.
#[proc_macro_attribute]
pub fn entry(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as syn::ItemFn);
    entry::entry(attr, item)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
