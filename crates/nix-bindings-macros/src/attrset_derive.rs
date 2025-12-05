use proc_macro2::TokenStream;
use syn::DeriveInput;

#[inline]
pub(crate) fn expand(_input: DeriveInput) -> syn::Result<TokenStream> {
    Ok(TokenStream::new())
}
