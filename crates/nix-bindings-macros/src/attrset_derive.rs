use proc_macro2::{Span, TokenStream};
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

    Ok(quote! {
        impl #impl_generics ::nix_bindings::attrset::derive::DerivedAttrset for #name #ty_generics #where_clause {
            const KEYS: &'static [&'static ::core::ffi::CStr] = &[];
            const MIGHT_SKIP_IDXS: &'static [u32] = &[];

            #[inline]
            fn should_skip(&self, #field_idx: u32) -> bool {
                match #field_idx {
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
                    _ => panic!("field index {} out of bounds", #field_idx),
                }
            }
        }
    })
}
