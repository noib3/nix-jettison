use std::ffi::CString;

use proc_macro2::{Literal, TokenStream};
use quote::{ToTokens, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::token::Comma;
use syn::{Token, braced};

use crate::list::Value;

#[inline]
pub(crate) fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let Attrset { keys, values } = syn::parse2(input)?;

    Ok(quote! {
        ::nix_bindings::attrset::LiteralAttrset::new(
            (#keys),
            (#values)
        )
    })
}

struct Attrset {
    keys: Punctuated<Key, Comma>,
    values: Punctuated<Value, Comma>,
}

enum Key {
    Literal(proc_macro2::Literal),
    Expr(syn::Expr),
}

impl Parse for Attrset {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut keys = Punctuated::new();
        let mut values = Punctuated::new();

        while !input.is_empty() {
            let key = input.parse()?;
            input.parse::<Token![:]>()?;
            let value = input.parse()?;

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

impl Parse for Key {
    #[inline]
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // If the key is wrapped in braces, parse it as an expression.
        if input.peek(syn::token::Brace) {
            let content;
            braced!(content in input);
            let expr: syn::Expr = content.parse()?;
            Ok(Self::Expr(expr))
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
            Ok(Self::Literal(Literal::c_string(&c_string)))
        }
    }
}

impl ToTokens for Key {
    #[inline]
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Self::Literal(lit) => tokens.extend(quote! {
                // SAFETY: valid UTF-8.
                unsafe { ::nix_bindings::Utf8CStr::new_unchecked(#lit) }
            }),
            Self::Expr(expr) => tokens.extend(quote! { { #expr } }),
        }
    }
}
