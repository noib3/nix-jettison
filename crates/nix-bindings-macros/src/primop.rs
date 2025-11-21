use proc_macro2::TokenStream;
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
            const DOCS: &'static ::core::ffi::CStr = unsafe {
                ::core::ffi::CStr::from_bytes_with_nul_unchecked(
                    concat!(#docs, "\0").as_bytes()
                )
            };

            const NAME: &'static ::nix_bindings::Utf8CStr = unsafe {
                ::nix_bindings::Utf8CStr::new_unchecked(
                    ::core::ffi::CStr::from_bytes_with_nul_unchecked(
                        concat!(#camel_case_name, "\0").as_bytes()
                    )
                )
            };

            const NEW: &'static Self = &#constructor;
        }
    })
}

fn camel_case_name(input: &DeriveInput) -> syn::Result<impl ToTokens> {
    let mut struct_name = input.ident.to_string();

    if struct_name.starts_with(|ch: char| ch.is_ascii_uppercase()) {
        // SAFETY: we just checked that the first byte is ASCII.
        let first_byte = unsafe { &mut struct_name.as_bytes_mut()[0] };
        *first_byte = first_byte.to_ascii_lowercase();
        Ok(struct_name)
    } else {
        Err(syn::Error::new(
            input.ident.span(),
            "PrimOp struct name must UpperCamelCase",
        ))
    }
}

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

fn docs(input: &DeriveInput) -> syn::Result<impl ToTokens> {
    let mut docs = String::new();

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
            if !docs.is_empty() {
                docs.push('\n');
            }
            docs.push_str(doc_line.strip_prefix(' ').unwrap_or(&doc_line));
        }
    }

    if docs.is_empty() {
        Err(syn::Error::new(
            input.ident.span(),
            "PrimOp derive requires a doc comment on the struct",
        ))
    } else {
        Ok(docs)
    }
}
