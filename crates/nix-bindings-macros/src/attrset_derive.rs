use std::ffi::CString;

use proc_macro2::{Literal, Span, TokenStream};
use quote::quote;
use syn::parse::Parse;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    Attribute,
    Data,
    DeriveInput,
    Expr,
    Fields,
    FieldsNamed,
    Token,
    WherePredicate,
};

use crate::try_from_value::{AttributePosition, Rename};

const MACRO_NAME: &str = "Attrset";

#[allow(clippy::too_many_lines)]
#[inline]
pub(crate) fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let attrs = Attributes::parse(&input.attrs, AttributePosition::Struct)?;

    let fields = named_fields(&input)?
        .named
        .iter()
        .map(|field| Field::new(field, &attrs))
        .collect::<syn::Result<Vec<_>>>()?;

    let keys = fields.iter().map(|field| &field.key_name);

    let might_skip_idxs =
        fields.iter().enumerate().filter_map(|(idx, field)| {
            if field.skip_expr.is_some() {
                Some(Literal::u32_unsuffixed(idx as u32))
            } else {
                None
            }
        });

    let should_skip_arms =
        fields.iter().enumerate().filter_map(|(idx, field)| {
            field.skip_expr.as_ref().map(|skip_expr| {
                let idx_lit = Literal::u32_unsuffixed(idx as u32);
                quote! { #idx_lit => #skip_expr, }
            })
        });

    let field_idx = syn::Ident::new("__field_idx", Span::call_site());
    let fun = syn::Ident::new("__fun", Span::call_site());
    let ctx = syn::Ident::new("__ctx", Span::call_site());

    let with_value_arms = fields.iter().enumerate().map(|(idx, field)| {
        let idx_lit = Literal::u32_unsuffixed(idx as u32);
        let with_value_expr = &field.with_value_expr;
        quote! { #idx_lit => #fun.call(#with_value_expr, #ctx), }
    });

    let (impl_generics, ty_generics, where_clause) =
        input.generics.split_for_impl();

    let struct_name = &input.ident;

    let extended_where_clause = if !attrs.bounds.is_empty() {
        let mut predicates =
            where_clause.map(|wc| wc.predicates.clone()).unwrap_or_default();
        predicates.extend(attrs.bounds.iter().cloned());
        quote! { where #predicates }
    } else {
        quote! { #where_clause }
    };

    Ok(quote! {
        impl #impl_generics ::nix_bindings::attrset::derive::DerivedAttrset for #struct_name #ty_generics #extended_where_clause {
            const KEYS: &'static [&'static ::core::ffi::CStr] = &[#(#keys),*];
            const MIGHT_SKIP_IDXS: &'static [u32] = &[#(#might_skip_idxs),*];

            #[inline]
            fn should_skip(&self, #field_idx: u32) -> bool {
                match #field_idx {
                    #(#should_skip_arms)*
                    _ => false,
                }
            }

            #[inline]
            fn with_value<'ctx, 'eval, T>(
                &self,
                #field_idx: u32,
                #fun: impl ::nix_bindings::value::FnOnceValue<T, &'ctx mut ::nix_bindings::prelude::Context<'eval>>,
                #ctx: &'ctx mut ::nix_bindings::prelude::Context<'eval>,
            ) -> T {
                match #field_idx {
                    #(#with_value_arms)*
                    _ => panic!("field index {} out of bounds", #field_idx),
                }
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

#[derive(Clone, Default)]
struct Attributes {
    rename: Option<Rename>,
    skip_if: Option<Expr>,
    with_value: Option<Expr>,
    bounds: Vec<WherePredicate>,
}

struct Field {
    key_name: Literal,
    skip_expr: Option<TokenStream>,
    with_value_expr: TokenStream,
}

impl Attributes {
    #[allow(clippy::too_many_lines)]
    fn parse(attrs: &[Attribute], pos: AttributePosition) -> syn::Result<Self> {
        let mut this = Self::default();

        for attr in attrs {
            if !attr.path().is_ident("attrset") {
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
                } else if meta.path.is_ident("skip_if") {
                    this.skip_if = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("with_value") {
                    match pos {
                        AttributePosition::Struct => {
                            return Err(meta.error(
                                "`with_value` attribute is only allowed on \
                                 struct fields",
                            ));
                        },
                        AttributePosition::Field => {
                            this.with_value = Some(meta.value()?.parse()?);
                        },
                    }
                } else if meta.path.is_ident("bounds") {
                    match pos {
                        AttributePosition::Struct => {
                            meta.input.parse::<Token![=]>()?;
                            let content;
                            syn::braced!(content in meta.input);
                            let bounds: Punctuated<WherePredicate, Token![,]> =
                                content
                                    .parse_terminated(Parse::parse, Token![,])?;
                            this.bounds.extend(bounds);
                        },
                        AttributePosition::Field => {
                            return Err(meta.error(
                                "`bounds` attribute is only allowed on structs",
                            ));
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

impl Field {
    fn new(field: &syn::Field, struct_attrs: &Attributes) -> syn::Result<Self> {
        let field_attrs =
            Attributes::parse(&field.attrs, AttributePosition::Field)?;

        let rename =
            field_attrs.rename.as_ref().or(struct_attrs.rename.as_ref());

        let skip_if =
            field_attrs.skip_if.as_ref().or(struct_attrs.skip_if.as_ref());

        let field_name = field.ident.as_ref().expect("fields are named");

        let mut key_name_str = field_name.to_string();

        if let Some(rename) = rename {
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

        let skip_expr = skip_if.map(|skip_expr| {
            quote! { (#skip_expr)(&self.#field_name) }
        });

        let with_value_expr = if let Some(with_value_expr) =
            field_attrs.with_value.as_ref()
        {
            quote! { (#with_value_expr)(self) }
        } else {
            quote! { ::nix_bindings::value::ToValue::to_value(&self.#field_name) }
        };

        Ok(Self { key_name, skip_expr, with_value_expr })
    }
}
