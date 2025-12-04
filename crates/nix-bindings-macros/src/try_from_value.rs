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

const MACRO_NAME: &str = "TryFromValue";

#[inline]
pub(crate) fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let attrs = Attributes::parse(&input.attrs)?;
    let fields = named_fields(&input)?;

    let attrset = Ident::new("__attrset", Span::call_site());
    let value = Ident::new("__value", Span::call_site());
    let ctx = Ident::new("__ctx", Span::call_site());
    let lifetime: LifetimeParam = parse_quote!('a);

    let try_from_attrset_impl =
        try_from_attrset_impl(&attrs, fields, &attrset, &ctx)?;
    let lifetime_generic = crate::args::lifetime_generic(&input, &lifetime)?;
    let struct_name = &input.ident;

    Ok(quote! {
        impl<#lifetime> ::nix_bindings::value::TryFromValue<::nix_bindings::value::NixValue<#lifetime>> for #struct_name #lifetime_generic {
            #[inline]
            fn try_from_value(
                #value: ::nix_bindings::value::NixValue<#lifetime>,
                #ctx: &mut ::nix_bindings::context::Context,
            ) -> ::nix_bindings::error::Result<Self> {
                let #attrset = ::nix_bindings::attrset::NixAttrset::try_from_value(
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
    attrs: &Attributes,
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
        let mut key_name_str = field.to_string();

        if let Some(rename) = &attrs.rename_all {
            rename.apply(&mut key_name_str);
        }

        let key_name = CString::new(key_name_str)
            .map_err(|err| {
                syn::Error::new(
                    field.span(),
                    format_args!("invalid field name: {err}"),
                )
            })
            .map(|name| Literal::c_string(&name))?;

        fields_initializers.extend(quote! {
            let #field = #attrset.get(#key_name, #ctx)?;
        })
    }

    Ok(quote! {
        #fields_initializers
        Ok(Self { #fields_list })
    })
}

struct Attributes {
    rename_all: Option<RenameAll>,
}

enum RenameAll {
    CamelCase,
}

impl Attributes {
    fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut this = Self { rename_all: None };

        for attr in attrs {
            if !attr.path().is_ident("try_from") {
                continue;
            }

            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename_all") {
                    this.rename_all = Some(RenameAll::parse(meta)?);
                    Ok(())
                } else {
                    Err(meta.error("unsupported attribute"))
                }
            })?;
        }

        Ok(this)
    }
}

impl RenameAll {
    fn apply(&self, field_name: &mut String) {
        match self {
            Self::CamelCase => to_camel_case(field_name),
        }
    }

    fn parse(meta: ParseNestedMeta<'_>) -> syn::Result<Self> {
        let lit = meta.value()?.parse::<Literal>()?;
        let lit_str = lit.to_string();
        let value = lit_str.trim_matches('"');

        match value {
            "camelCase" => Ok(Self::CamelCase),
            _ => Err(syn::Error::new(
                lit.span(),
                format_args!("unsupported rename_all value: {value}"),
            )),
        }
    }
}

fn to_camel_case(field_name: &mut String) {
    debug_assert!(!field_name.contains(' '));

    let mut offset = 0;

    let mut replace_buffer = [b' ', b' '];

    while offset < field_name.len() {
        let Some((component, rest)) = field_name[offset..].split_once('_')
        else {
            break;
        };

        offset += component.len();

        let Some(char_after_underscore) = rest.chars().next() else {
            // Trailing underscore.
            break;
        };

        let replacement = if char_after_underscore.is_ascii() {
            let uppercased = char_after_underscore.to_ascii_uppercase();
            replace_buffer[1] = uppercased as u8;
            str::from_utf8(&replace_buffer).expect("valid utf8")
        } else {
            " "
        };

        let replace_end = offset + 1 + (replacement.len() > 1) as usize;
        field_name.replace_range(offset..replace_end, replacement);
        offset += 1 + char_after_underscore.len_utf8();
    }

    field_name.retain(|ch| ch != ' ');
}
