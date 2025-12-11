use core::ptr::NonNull;

use nix_bindings_sys as sys;

use crate::prelude::{Context, Entrypoint};

pub type EntrypointFun = for<'a> fn(&mut Context<Entrypoint>);

#[doc(hidden)]
#[inline]
pub unsafe fn entry(entrypoint: EntrypointFun) {
    match NonNull::new(unsafe { sys::c_context_create() }) {
        Some(ctx) => {
            entrypoint(&mut Context::new(ctx, Entrypoint {}));
            unsafe { sys::c_context_free(ctx.as_ptr()) };
        },
        None => panic!("couldn't allocate new 'nix_c_context'"),
    }
}
