use core::iter;
use std::ffi::CString;

use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::{ToTokens, quote};
use syn::meta::ParseNestedMeta;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::token::Comma;
use syn::{
    Attribute,
    Data,
    DeriveInput,
    Fields,
    FieldsNamed,
    LifetimeParam,
    parse_quote,
};

#[inline]
pub(crate) fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let attrs = ArgsAttributes::parse(&input.attrs)?;
    let fields = named_fields(&input)?;

    let args = Ident::new("__args_list", Span::call_site());
    let ctx = Ident::new("__ctx", Span::call_site());
    let lifetime: LifetimeParam = parse_quote!('a);

    let args_list = args_list(&attrs, fields)?;
    let from_raw_impl = from_raw_impl(&attrs, fields, &args, &ctx);
    let lifetime_generic = lifetime_generic(&input, &lifetime)?;
    let struct_name = &input.ident;

    Ok(quote! {
        impl<#lifetime> ::nix_bindings::primop::Args<#lifetime> for #struct_name #lifetime_generic {
            const NAMES: &'static [*const ::core::ffi::c_char] = &[#args_list];

            #[inline]
            unsafe fn from_raw(
                #args: ::nix_bindings::primop::ArgsList<#lifetime>,
                #ctx: &mut ::nix_bindings::context::Context,
            ) -> ::nix_bindings::error::Result<Self> {
                #from_raw_impl
            }
        }
    })
}

struct ArgsAttributes {
    flatten: bool,
    name: Option<CString>,
    span: Span,
}

enum ArgsAttribute {
    Flatten,
    Name(CString),
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

fn args_list(
    struct_attrs: &ArgsAttributes,
    fields: &FieldsNamed,
) -> syn::Result<impl ToTokens> {
    let mut fields = if struct_attrs.flatten {
        let name = struct_attrs.name.as_ref().ok_or_else(|| {
            syn::Error::new(
                struct_attrs.span,
                "`name` attribute is required when `flatten` is used",
            )
        })?;

        let c_str = Literal::c_string(name);

        Punctuated::<_, Comma>::from_iter(iter::once(
            quote! { #c_str.as_ptr() },
        ))
    } else {
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
                syn::Result::Ok(quote! { #c_str.as_ptr() })
            })
            .collect::<Result<_, _>>()?
    };

    fields.push_punct(Default::default());
    fields.push_value(quote! { ::core::ptr::null() });

    Ok(fields)
}

fn from_raw_impl(
    struct_attrs: &ArgsAttributes,
    fields: &FieldsNamed,
    args: &Ident,
    ctx: &Ident,
) -> impl ToTokens {
    if struct_attrs.flatten {
        quote! {
            // SAFETY: up to the caller.
            unsafe { #args.get(0, #ctx) }
        }
    } else {
        let fields_list = fields
            .named
            .iter()
            .map(|field| field.ident.as_ref().expect("fields are named"))
            .collect::<Punctuated<_, Comma>>();

        let fields_initializers =
            fields_list.iter().enumerate().map(move |(idx, &field)| {
                let idx = idx as u8;
                quote! {
                    // SAFETY: up to the caller.
                    let #field = unsafe { #args.get(#idx, #ctx)? };
                }
            });

        quote! {
            #(#fields_initializers)*
            Ok(Self { #fields_list })
        }
    }
}

pub(crate) fn lifetime_generic(
    input: &DeriveInput,
    lifetime: &LifetimeParam,
) -> syn::Result<impl ToTokens> {
    match input.generics.params.iter().fold(
        (0, 0),
        |(num_total, num_lifetimes), r#gen| {
            let is_lifetime = matches!(r#gen, syn::GenericParam::Lifetime(_));
            (num_total + 1, num_lifetimes + (is_lifetime as usize))
        },
    ) {
        (0, 0) => Ok(None),
        (1, 1) => Ok(Some(quote! { <#lifetime> })),
        _ => Err(syn::Error::new(
            input.generics.span(),
            "Args can only have zero or one lifetime generic parameter",
        )),
    }
}

impl ArgsAttributes {
    fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut this =
            Self { flatten: false, name: None, span: Span::call_site() };

        for attr in attrs {
            if !attr.path().is_ident("args") {
                continue;
            }

            attr.parse_nested_meta(|meta| {
                match ArgsAttribute::parse(meta)? {
                    ArgsAttribute::Flatten => this.flatten = true,
                    ArgsAttribute::Name(cstring) => this.name = Some(cstring),
                }
                Ok(())
            })?;

            this.span = attr.span();
        }

        Ok(this)
    }
}

impl ArgsAttribute {
    fn parse(meta: ParseNestedMeta<'_>) -> syn::Result<Self> {
        if meta.path.is_ident("flatten") {
            return Ok(Self::Flatten);
        }

        if meta.path.is_ident("name") {
            let lit = meta.value()?.parse::<Literal>()?;
            let lit_str = lit.to_string();
            let name_str = lit_str.trim_matches('"');
            let cstring = CString::new(name_str).map_err(|err| {
                syn::Error::new(
                    lit.span(),
                    format_args!("invalid name: {err}"),
                )
            })?;
            return Ok(Self::Name(cstring));
        }

        Err(meta.error("unsupported attribute"))
    }
}
