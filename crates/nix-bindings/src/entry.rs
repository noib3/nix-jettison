use core::ptr::NonNull;

use crate::prelude::{Context, Entrypoint};

pub type EntrypointFun = for<'a> fn(&mut Context<Entrypoint>);

#[doc(hidden)]
#[inline]
pub unsafe fn entry(entrypoint: EntrypointFun) {
    match NonNull::new(unsafe { nix_bindings_sys::c_context_create() }) {
        Some(ctx) => {
            entrypoint(&mut Context::new(ctx, Entrypoint {}));
            unsafe { nix_bindings_sys::c_context_free(ctx.as_ptr()) };
        },
        None => panic!("couldn't allocate new 'nix_c_context'"),
    }
}
