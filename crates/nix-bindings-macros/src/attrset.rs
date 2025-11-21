use std::ffi::CString;

use proc_macro2::{Literal, TokenStream};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::token::Comma;
use syn::{Token, braced};

#[inline]
pub(crate) fn attrset(input: TokenStream) -> syn::Result<TokenStream> {
    let Attrset { keys, values } = syn::parse2(input)?;

    Ok(quote! {
        ::nix_bindings::attrset::LiteralAttrset::new(
            (#keys),
            (#values)
        )
    })
}

struct Attrset {
    keys: Punctuated<TokenStream, Comma>,
    values: Punctuated<syn::Expr, Comma>,
}

impl Parse for Attrset {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut keys = Punctuated::new();
        let mut values = Punctuated::new();

        while !input.is_empty() {
            // If the key is wrapped in braces, parse it as an expression.
            let key = if input.peek(syn::token::Brace) {
                let content;
                braced!(content in input);
                let expr: syn::Expr = content.parse()?;
                quote! { #expr }
            }
            // Otherwise, parse it as an ident and convert to c-string literal.
            else {
                let ident: syn::Ident = input.parse()?;
                let ident_str = ident.to_string();
                let c_string = CString::new(ident_str).map_err(|_| {
                    syn::Error::new(
                        ident.span(),
                        "attrset key cannot contain NUL byte",
                    )
                })?;
                let c_literal = Literal::c_string(&c_string);
                quote! {
                    // SAFETY: valid UTF-8.
                    unsafe { ::nix_bindings::Utf8CStr::new_unchecked(#c_literal) }
                }
            };

            input.parse::<Token![:]>()?;

            let value: syn::Expr = input.parse()?;

            keys.push(key);
            values.push(value);

            if input.peek(Comma) {
                let comma = input.parse()?;
                keys.push_punct(comma);
                values.push_punct(comma);
            }
        }

        Ok(Self { keys, values })
    }
}
