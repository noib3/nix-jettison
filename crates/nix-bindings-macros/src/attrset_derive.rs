use proc_macro2::{Literal, Span, TokenStream};
use quote::quote;
use syn::DeriveInput;

#[inline]
pub(crate) fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;

    let (impl_generics, ty_generics, where_clause) =
        input.generics.split_for_impl();

    let field_idx = syn::Ident::new("__field_idx", Span::call_site());
    let fun = syn::Ident::new("__fun", Span::call_site());
    let ctx = syn::Ident::new("__ctx", Span::call_site());

    let fields = fields();

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

    let with_value_arms = fields.iter().enumerate().map(|(idx, field)| {
        let idx_lit = Literal::u32_unsuffixed(idx as u32);
        let with_value_expr = &field.with_value_expr;
        quote! { #idx_lit => #fun.call(#with_value_expr, #ctx), }
    });

    Ok(quote! {
        impl #impl_generics ::nix_bindings::attrset::derive::DerivedAttrset for #name #ty_generics #where_clause {
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

fn fields() -> Vec<Field> {
    vec![]
}

struct Field {
    key_name: Literal,
    skip_expr: Option<TokenStream>,
    with_value_expr: TokenStream,
}
