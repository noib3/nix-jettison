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
    Expr,
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

        let field_name = field.ident.as_ref().expect("fields are named");

        let mut key_name_str = field_name.to_string();

        if let Some(rename) =
            field_attrs.rename.as_ref().or(struct_attrs.rename.as_ref())
        {
            rename.clone().apply(&mut key_name_str);
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

        field_initializers.extend(
            if struct_attrs.default || field_attrs.default {
                quote! {
                    let #field_name = #attrset.get_opt(#key_name, #ctx)?
                        .unwrap_or_default();
                }
            } else if let Some(with_expr) = field_attrs.with {
                quote! {
                    let #field_name = {
                        let __value = #attrset.get::<::nix_bindings::value::NixValue>(#key_name, #ctx)?;
                        (#with_expr)(__value, #ctx)?
                    };
                }
            } else {
                quote! {
                    let #field_name = #attrset.get(#key_name, #ctx)?;
                }
            },
        );
    }

    Ok(quote! {
        #field_initializers
        Ok(Self { #field_names })
    })
}

#[derive(Clone, Default)]
struct Attributes {
    rename: Option<Rename>,
    default: bool,
    with: Option<Expr>,
}

#[derive(Copy, Clone)]
pub(crate) enum AttributePosition {
    Field,
    Struct,
}

#[derive(Clone)]
pub(crate) enum Rename {
    CamelCase,
    Replace(String),
}

impl Attributes {
    #[allow(clippy::too_many_lines)]
    fn parse(attrs: &[Attribute], pos: AttributePosition) -> syn::Result<Self> {
        let mut this = Self::default();

        for attr in attrs {
            if !attr.path().is_ident("try_from") {
                continue;
            }

            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename_all") {
                    match pos {
                        AttributePosition::Struct => {
                            this.rename = Some(Rename::parse(meta, pos)?);
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
                                "`rename` attribute is only allowed on struct \
                                 fields",
                            ));
                        },
                        AttributePosition::Field => {
                            this.rename = Some(Rename::parse(meta, pos)?);
                        },
                    }
                } else if meta.path.is_ident("default") {
                    if this.with.is_some() {
                        return Err(meta.error(
                            "`with` and `default` attributes cannot be used \
                             together",
                        ));
                    }
                    this.default = true;
                } else if meta.path.is_ident("with") {
                    if this.default {
                        return Err(meta.error(
                            "`with` and `default` attributes cannot be used \
                             together",
                        ));
                    }

                    match pos {
                        AttributePosition::Struct => {
                            return Err(meta.error(
                                "`with` attribute is only allowed on struct \
                                 fields",
                            ));
                        },
                        AttributePosition::Field => {
                            this.with = Some(meta.value()?.parse()?);
                        },
                    }
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
    pub(crate) fn apply(self, field_name: &mut String) {
        match self {
            Self::CamelCase => to_camel_case(field_name),
            Self::Replace(s) => *field_name = s,
        }
    }

    pub(crate) fn parse(
        meta: ParseNestedMeta<'_>,
        pos: AttributePosition,
    ) -> syn::Result<Self> {
        let value = meta.value()?;

        let fork = value.fork();
        if let Ok(ident) = fork.parse::<syn::Ident>() {
            value.parse::<syn::Ident>()?;
            match ident.to_string().as_str() {
                "camelCase" => return Ok(Self::CamelCase),
                _ => {
                    return Err(syn::Error::new(
                        ident.span(),
                        format_args!("unsupported rename value: {}", ident),
                    ));
                },
            }
        }

        let lit: Literal = value.parse()?;
        let lit_str = lit.to_string();
        let value = lit_str.trim_matches('"');

        match pos {
            AttributePosition::Field => Ok(Self::Replace(value.to_string())),
            AttributePosition::Struct => Err(syn::Error::new(
                lit.span(),
                "literal string renames are only allowed on struct fields",
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
