use std::ffi::CString;

use proc_macro2::{Literal, TokenStream};
use quote::{ToTokens, quote};
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::token::Comma;
use syn::{Attribute, Token, braced};

use crate::list::Value;

#[inline]
pub(crate) fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let Attrset { entries } = syn::parse2(input)?;

    let mut keys = TokenStream::new();
    let mut values = TokenStream::new();
    let comma = <Token![,]>::default();

    for (idx, entry) in entries.iter().enumerate() {
        // Add the entry's attributes to both keys and values.
        for attr in &entry.attrs {
            attr.to_tokens(&mut keys);
            attr.to_tokens(&mut values);
        }

        entry.key.to_tokens(&mut keys);
        entry.value.to_tokens(&mut values);

        // Add a comma if this is not the last entry.
        if idx + 1 < entries.len() {
            comma.to_tokens(&mut keys);
            comma.to_tokens(&mut values);
        }
    }

    Ok(quote! {
        ::nix_bindings::attrset::LiteralAttrset::new(
            (#keys),
            (#values)
        )
    })
}

struct Attrset {
    entries: Vec<AttrsetEntry>,
}

struct AttrsetEntry {
    attrs: Vec<Attribute>,
    key: Key,
    value: Value,
}

enum Key {
    Literal(proc_macro2::Literal),
    Expr(syn::Expr),
}

impl Parse for Attrset {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut entries = Vec::new();

        while !input.is_empty() {
            // Parse attributes (e.g., #[cfg(...)]).
            let attrs = input.call(Attribute::parse_outer)?;

            // Parse key.
            let key = input.parse()?;
            input.parse::<Token![:]>()?;

            // Parse value.
            let value = input.parse()?;

            entries.push(AttrsetEntry { attrs, key, value });

            // Parse optional comma.
            if input.peek(Comma) {
                input.parse::<Comma>()?;
            }
        }

        Ok(Self { entries })
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
        // Otherwise, parse it as an ident (including keywords) and convert to
        // c-string literal.
        else {
            let ident = input.call(syn::Ident::parse_any)?;
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
