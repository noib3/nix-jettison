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
    let attrs = Attributes::parse(&input.attrs, AttributePosition::Struct)?;
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
    struct_attrs: &Attributes,
    fields: &FieldsNamed,
    attrset: &Ident,
    ctx: &Ident,
) -> syn::Result<impl ToTokens> {
    let mut field_names = Punctuated::<_, Comma>::new();
    let mut field_initializers = TokenStream::new();

    for field in fields.named.iter() {
        let field_attrs =
            Attributes::parse(&field.attrs, AttributePosition::Field)?;

        let attrs = struct_attrs.combine(field_attrs);

        let field_name = field.ident.as_ref().expect("fields are named");

        let mut key_name_str = field_name.to_string();

        if let Some(rename) = &attrs.rename {
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

        field_names.push(field_name);

        field_initializers.extend(if attrs.default {
            quote! {
                let #field_name = #attrset.get_opt(#key_name, #ctx)?
                    .unwrap_or_default();
            }
        } else {
            quote! {
                let #field_name = #attrset.get(#key_name, #ctx)?;
            }
        });
    }

    Ok(quote! {
        #field_initializers
        Ok(Self { #field_names })
    })
}

#[derive(Copy, Clone)]
struct Attributes {
    rename: Option<Rename>,
    default: bool,
}

#[derive(Copy, Clone)]
enum AttributePosition {
    Field,
    Struct,
}

#[derive(Copy, Clone)]
enum Rename {
    CamelCase,
}

impl Attributes {
    fn combine(self, other: Self) -> Self {
        Self {
            rename: other.rename.or(self.rename),
            default: self.default || other.default,
        }
    }

    fn parse(
        attrs: &[Attribute],
        pos: AttributePosition,
    ) -> syn::Result<Self> {
        let mut this = Self { rename: None, default: false };

        for attr in attrs {
            if !attr.path().is_ident("try_from") {
                continue;
            }

            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename_all") {
                    match pos {
                        AttributePosition::Struct => {
                            this.rename = Some(Rename::parse(meta)?);
                        },
                        AttributePosition::Field => {
                            return Err(meta.error(
                                "`rename_all` attribute is only allowed on \
                                 structs",
                            ));
                        },
                    }
                } else if meta.path.is_ident("rename") {
                    match pos {
                        AttributePosition::Struct => {
                            return Err(meta.error(
                                "`rename` attribute is only allowed on \
                                 struct fields",
                            ));
                        },
                        AttributePosition::Field => {
                            this.rename = Some(Rename::parse(meta)?);
                        },
                    }
                } else if meta.path.is_ident("default") {
                    this.default = true;
                } else {
                    return Err(meta.error("unsupported attribute"));
                }

                Ok(())
            })?;
        }

        Ok(this)
    }
}

impl Rename {
    fn apply(self, field_name: &mut String) {
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
