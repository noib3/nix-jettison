use core::ffi::{CStr, c_char};

use nix_bindings_sys as sys;

use crate::{Context, Result, Value};

const MAX_ARITY: u8 = 8;

/// TODO: docs.
pub trait PrimOp: PrimOpFun + Sized {
    /// TODO: docs.
    const NAME: &'static CStr;

    /// TODO: docs.
    const DOCS: &'static CStr;

    #[doc(hidden)]
    #[inline]
    unsafe fn alloc(self, ctx: *mut sys::c_context) -> *mut sys::PrimOp {
        debug_assert!(!ctx.is_null());

        #[allow(path_statements)]
        Self::Args::CHECKS;

        unsafe {
            sys::alloc_primop(
                ctx,
                Self::c_fun(),
                Self::Args::ARITY.into(),
                Self::NAME.as_ptr(),
                Self::Args::LIST.as_ptr().cast_mut(),
                Self::DOCS.as_ptr(),
                // This is a leak, but it's ok because it only happens once in
                // the lifetime of the plugin.
                Box::into_raw(Box::new(self)).cast(),
            )
        }
    }
}

/// TODO: docs.
pub trait PrimOpFun: 'static {
    /// TODO: docs.
    type Args: Args;

    /// TODO: docs.
    fn call(&self, args: Self::Args, ctx: &mut Context) -> Result<impl Value>;

    #[doc(hidden)]
    fn c_fun() -> sys::PrimOpFun {
        todo!();
    }
}

/// TODO: docs.
pub trait Args {
    // Compile-time checks of several invariants.
    #[doc(hidden)]
    const CHECKS: () = {
        assert!(Self::ARITY <= MAX_ARITY);
        assert!(Self::ARITY as usize + 1 == Self::LIST.len());
        assert!(Self::LIST.last().unwrap().is_null());
    };

    #[doc(hidden)]
    const ARITY: u8 = Self::LIST.len() as u8 - 1;

    /// TODO: docs.
    const LIST: &'static [*const c_char];
}
