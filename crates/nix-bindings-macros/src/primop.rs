use std::ffi::CString;

use proc_macro2::{Literal, TokenStream};
use quote::{ToTokens, quote};
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields};

#[inline]
pub(crate) fn primop(input: DeriveInput) -> syn::Result<TokenStream> {
    let camel_case_name = camel_case_name(&input)?;
    let constructor = constructor(&input)?;
    let docs = docs(&input)?;
    let struct_name = &input.ident;

    Ok(quote! {
        impl ::nix_bindings::primop::PrimOp for #struct_name {
            const DOCS: &'static ::core::ffi::CStr = #docs;

            const NAME: &'static ::nix_bindings::Utf8CStr = unsafe {
                ::nix_bindings::Utf8CStr::new_unchecked(#camel_case_name)
            };

            const NEW: &'static Self = &#constructor;
        }
    })
}

#[inline]
fn camel_case_name(input: &DeriveInput) -> syn::Result<impl ToTokens> {
    let mut struct_name = input.ident.to_string().into_bytes();

    if let Some(first_byte) = struct_name.get_mut(0)
        && first_byte.is_ascii_uppercase()
    {
        *first_byte = first_byte.to_ascii_lowercase();
    }

    let struct_name = CString::new(struct_name).map_err(|_| {
        syn::Error::new(input.ident.span(), "struct name contains NUL byte")
    })?;

    Ok(Literal::c_string(&struct_name))
}

#[inline]
fn constructor(input: &DeriveInput) -> syn::Result<impl ToTokens> {
    let r#struct = match &input.data {
        Data::Struct(str) => str,
        Data::Enum(_) => {
            return Err(syn::Error::new(
                input.span(),
                "PrimOp cannot be derived for enums",
            ));
        },
        Data::Union(_) => {
            return Err(syn::Error::new(
                input.span(),
                "PrimOp cannot be derived for unions",
            ));
        },
    };

    match &r#struct.fields {
        Fields::Unit => Ok(quote! { Self }),

        Fields::Named(fields) if fields.named.is_empty() => {
            Ok(quote! { Self {} })
        },

        Fields::Unnamed(fields) if fields.unnamed.is_empty() => {
            Ok(quote! { Self() })
        },

        _ => Err(syn::Error::new(
            input.span(),
            "PrimOp can only be derived for structs with no fields (unit \
             structs, empty named structs, or empty tuple structs)",
        )),
    }
}

#[inline]
fn docs(input: &DeriveInput) -> syn::Result<impl ToTokens> {
    let mut docs = String::new();
    let mut is_first_line = true;

    for attr in &input.attrs {
        if attr.path().is_ident("doc")
            && let syn::Meta::NameValue(meta) = &attr.meta
            && let syn::Expr::Lit(expr_lit) = &meta.value
            && let syn::Lit::Str(lit_str) = &expr_lit.lit
        {
            let doc_line = lit_str.value();
            if doc_line.contains('\0') {
                return Err(syn::Error::new(
                    lit_str.span(),
                    "PrimOp doc comment cannot contain NUL byte",
                ));
            }
            if !is_first_line {
                docs.push('\n');
            }
            docs.push_str(doc_line.strip_prefix(' ').unwrap_or(&doc_line));
            is_first_line = false;
        }
    }

    if docs.is_empty() {
        Err(syn::Error::new(
            input.span(),
            "PrimOp derive requires a doc comment on the struct",
        ))
    } else {
        // SAFETY: we checked for NUL bytes while iterating over the
        // attributes.
        let docs = unsafe { CString::from_vec_unchecked(docs.into_bytes()) };
        Ok(Literal::c_string(&docs))
    }
}
