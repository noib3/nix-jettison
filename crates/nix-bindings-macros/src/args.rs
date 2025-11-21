use core::iter;
use std::ffi::CString;

use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::{ToTokens, quote};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::token::Comma;
use syn::{Data, DeriveInput, Fields, FieldsNamed};

#[inline]
pub(crate) fn args(input: DeriveInput) -> syn::Result<TokenStream> {
    let args = Ident::new("args", Span::call_site());
    let ctx = Ident::new("ctx", Span::call_site());

    let fields = named_fields(&input)?;
    let args_list = args_list(fields)?;
    let fields_initializers = fields_initializers(fields, &args, &ctx);
    let fields_list = fields_list(fields);
    let struct_name = &input.ident;

    Ok(quote! {
        impl ::nix_bindings::primop::Args for #struct_name {
            const NAMES: &'static [*const ::core::ffi::c_char] = &[#args_list];

            #[inline]
            unsafe fn from_raw(
                #args: ::core::ptr::NonNull<*mut ::nix_bindings::sys::Value>,
                #ctx: &mut ::nix_bindings::context::Context,
            ) -> ::nix_bindings::error::Result<Self> {
                 #(#fields_initializers)*
                Ok(Self { #fields_list })
            }
        }
    })
}

fn named_fields(input: &DeriveInput) -> syn::Result<&FieldsNamed> {
    let r#struct = match &input.data {
        Data::Struct(str) => str,
        Data::Enum(_) => {
            return Err(syn::Error::new(
                input.span(),
                "Args cannot be derived for enums",
            ));
        },
        Data::Union(_) => {
            return Err(syn::Error::new(
                input.span(),
                "Args cannot be derived for unions",
            ));
        },
    };

    match &r#struct.fields {
        Fields::Named(fields) => match fields.named.len() {
            0 => Err(syn::Error::new(
                input.span(),
                "Args can only be derived for structs with at least one \
                 named field",
            )),

            len if len > nix_bindings_sys::MAX_PRIMOP_ARITY as usize => {
                Err(syn::Error::new(
                    input.span(),
                    format_args!(
                        "In Nix, functions can have at most {} arguments, \
                         but this struct has {len} fields",
                        nix_bindings_sys::MAX_PRIMOP_ARITY
                    ),
                ))
            },

            _ => Ok(fields),
        },
        Fields::Unit | Fields::Unnamed(_) => Err(syn::Error::new(
            input.span(),
            "Args can only be derived for structs with named fields",
        )),
    }
}

fn args_list(fields: &FieldsNamed) -> syn::Result<impl ToTokens> {
    fields
        .named
        .iter()
        .map(|field| {
            let ident = field.ident.as_ref().expect("fields are named");
            let name = ident.to_string();
            let name = CString::new(name).map_err(|err| {
                syn::Error::new(
                    ident.span(),
                    format_args!("invalid field name: {err}"),
                )
            })?;
            let c_str = Literal::c_string(&name);
            Ok(quote! { #c_str.as_ptr() })
        })
        .chain(iter::once(Ok(quote! { ::core::ptr::null() })))
        .collect::<Result<Punctuated<_, Comma>, _>>()
}

fn fields_initializers(
    fields: &FieldsNamed,
    args: &Ident,
    ctx: &Ident,
) -> impl Iterator<Item = impl ToTokens> {
    fields.named.iter().enumerate().map(move |(idx, field)| {
        let ident = field.ident.as_ref().expect("fields are named");
        let ty = &field.ty;
        let idx = idx as u8;
        quote! {
            // SAFETY: up to the caller.
            let #ident = unsafe {
                #ctx.get_arg::<#ty>(#args, #idx)?
            };
        }
    })
}

fn fields_list(fields: &FieldsNamed) -> impl ToTokens {
    fields
        .named
        .iter()
        .map(|field| field.ident.as_ref().expect("fields are named"))
        .collect::<Punctuated<_, Comma>>()
}
