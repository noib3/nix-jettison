use std::ffi::CString;

use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::{ToTokens, quote};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::token::Comma;
use syn::{
    Data,
    DeriveInput,
    Fields,
    FieldsNamed,
    LifetimeParam,
    parse_quote,
};

const MACRO_NAME: &str = "TryFromValue";

#[inline]
pub(crate) fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let fields = named_fields(&input)?;

    let attrset = Ident::new("__attrset", Span::call_site());
    let value = Ident::new("__value", Span::call_site());
    let ctx = Ident::new("__ctx", Span::call_site());
    let lifetime: LifetimeParam = parse_quote!('a);

    let try_from_attrset_impl = try_from_attrset_impl(fields, &attrset, &ctx)?;
    let lifetime_generic = crate::args::lifetime_generic(&input, &lifetime)?;
    let struct_name = &input.ident;

    Ok(quote! {
        impl<#lifetime> ::nix_bindings::value::TryFromValue<::nix_bindings::value::ValuePointer<#lifetime>> for #struct_name #lifetime_generic {
            #[inline]
            fn try_from_value(
                #value: ::nix_bindings::value::ValuePointer<#lifetime>,
                #ctx: &mut ::nix_bindings::context::Context,
            ) -> ::nix_bindings::error::Result<Self> {
                let #attrset = ::nix_bindings::attrset::AnyAttrset::try_from_value(
                    #value, #ctx,
                )?;
                #try_from_attrset_impl
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
                format_args!("{MACRO_NAME} cannot be derived for enums"),
            ));
        },
        Data::Union(_) => {
            return Err(syn::Error::new(
                input.span(),
                format_args!("{MACRO_NAME} cannot be derived for unions"),
            ));
        },
    };

    match &r#struct.fields {
        Fields::Named(fields) => match fields.named.len() {
            0 => Err(syn::Error::new(
                input.span(),
                "struct must have at least one field",
            )),

            _ => Ok(fields),
        },
        Fields::Unit | Fields::Unnamed(_) => Err(syn::Error::new(
            input.span(),
            format_args!(
                "{MACRO_NAME} can only be derived for structs with named \
                 fields"
            ),
        )),
    }
}

fn try_from_attrset_impl(
    fields: &FieldsNamed,
    attrset: &Ident,
    ctx: &Ident,
) -> syn::Result<impl ToTokens> {
    let fields_list = fields
        .named
        .iter()
        .map(|field| field.ident.as_ref().expect("fields are named"))
        .collect::<Punctuated<_, Comma>>();

    let mut fields_initializers = TokenStream::new();

    for field in &fields_list {
        let field_name = CString::new(field.to_string())
            .map_err(|err| {
                syn::Error::new(
                    field.span(),
                    format_args!("invalid field name: {err}"),
                )
            })
            .map(|name| Literal::c_string(&name))?;

        fields_initializers.extend(quote! {
            let #field = #attrset.get(#field_name, #ctx)?;
        })
    }

    Ok(quote! {
        #fields_initializers
        Ok(Self { #fields_list })
    })
}
