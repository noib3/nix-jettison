use proc_macro2::TokenStream;
use quote::quote;
use syn::DeriveInput;

#[inline]
pub(crate) fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;

    let (impl_generics, ty_generics, where_clause) =
        input.generics.split_for_impl();

    // Extract type and lifetime parameters for the use<..> clause
    let type_params = input.generics.type_params().map(|tp| &tp.ident);
    let lifetime_params = input.generics.lifetimes().map(|lt| &lt.lifetime);

    Ok(quote! {
        impl #impl_generics ::nix_bindings::attrset::Attrset for #name #ty_generics #where_clause {
            #[inline]
            fn len(&self, ctx: &mut ::nix_bindings::prelude::Context) -> ::core::ffi::c_uint {
                ::core::todo!()
            }

            #[inline]
            fn pairs<'this, 'eval>(
                &'this self,
                ctx: &mut ::nix_bindings::prelude::Context<'eval>,
            ) -> impl ::nix_bindings::attrset::Pairs + use<'this, 'eval, #(#lifetime_params,)* #(#type_params),*> {
                struct TodoPairs;

                impl ::nix_bindings::attrset::Pairs for TodoPairs {
                    #[inline]
                    fn advance(
                        &mut self,
                        ctx: &mut ::nix_bindings::prelude::Context,
                    ) {
                        ::core::todo!()
                    }

                    #[inline]
                    fn is_exhausted(&self) -> bool{
                        ::core::todo!()
                    }

                    #[inline]
                    fn key(
                        &self,
                        ctx: &mut ::nix_bindings::prelude::Context,
                    ) -> &::core::ffi::CStr {
                        ::core::todo!()
                    }

                    #[inline]
                    fn with_value<'ctx, 'eval, T>(
                        &self,
                        fun: impl ::nix_bindings::value::FnOnceValue<T, &'ctx mut ::nix_bindings::prelude::Context<'eval>>,
                        ctx: &'ctx mut ::nix_bindings::prelude::Context<'eval>,
                    ) -> T {
                        ::core::todo!()
                    }
                }

                TodoPairs
            }

            #[inline]
            fn with_value<'ctx, 'eval, T>(
                &self,
                key: &::core::ffi::CStr,
                fun: impl ::nix_bindings::value::FnOnceValue<T, &'ctx mut ::nix_bindings::prelude::Context<'eval>>,
                ctx: &'ctx mut ::nix_bindings::prelude::Context<'eval>,
            ) -> ::core::option::Option<T> {
                ::core::todo!()
            }
        }
    })
}
