use core::ffi::{CStr, c_char, c_void};
use core::ptr::NonNull;

use nix_bindings_sys as sys;

use crate::{Context, EvalState, Result, TryIntoValue, Value};

const MAX_ARITY: u8 = 8;

/// TODO: docs.
pub trait PrimOp: PrimOpFun + Sized {
    /// TODO: docs.
    const NAME: &'static CStr;

    /// TODO: docs.
    const DOCS: &'static CStr;

    #[doc(hidden)]
    #[inline]
    unsafe fn alloc(self, ctx: NonNull<sys::c_context>) -> *mut sys::PrimOp {
        #[allow(path_statements)]
        Self::Args::CHECKS;

        let type_erased: Box<dyn TypeErasedPrimOpFun> = Box::new(self);

        unsafe {
            sys::alloc_primop(
                ctx.as_ptr(),
                Self::c_fun(),
                Self::Args::ARITY.into(),
                Self::NAME.as_ptr(),
                Self::Args::NAMES.as_ptr().cast_mut(),
                Self::DOCS.as_ptr(),
                // This is a leak, but it's ok because it only happens once in
                // the lifetime of the plugin.
                Box::into_raw(Box::new(type_erased)).cast(),
            )
        }
    }
}

/// TODO: docs.
pub trait PrimOpFun: 'static {
    /// TODO: docs.
    type Args: Args;

    /// TODO: docs.
    fn call<'a>(
        &'a self,
        args: Self::Args,
        ctx: &mut Context,
    ) -> impl TryIntoValue + use<'a, Self>;

    #[doc(hidden)]
    fn c_fun() -> sys::PrimOpFun {
        unsafe extern "C" fn wrapper(
            user_data: *mut c_void,
            ctx: *mut sys::c_context,
            state: *mut sys::EvalState,
            args: *mut *mut sys::Value,
            ret: *mut sys::Value,
        ) {
            // SAFETY:
            // - user_data was created in PrimOp::alloc from a
            //   *mut Box<dyn TypeErasedPrimOpFun>;
            // - the raw pointer is guaranteed to still point to valid memory
            //   because Box::from_raw is never called;
            // - TypeErasedPrimOpFun is bound to 'static;
            let primop: &dyn TypeErasedPrimOpFun =
                unsafe { &**(user_data as *mut Box<dyn TypeErasedPrimOpFun>) };

            let Some(ctx) = NonNull::new(ctx) else {
                panic!("received NULL `nix_c_context` pointer in primop call");
            };

            let Some(state) = NonNull::new(state) else {
                panic!("received NULL `EvalState` pointer in primop call");
            };

            unsafe {
                primop.call(
                    args,
                    ret,
                    &mut Context::new(ctx, EvalState::new(state)),
                )
            }
        }

        Some(wrapper)
    }
}

/// TODO: docs.
pub trait Args: Sized {
    // Compile-time checks of several invariants.
    #[doc(hidden)]
    const CHECKS: () = {
        assert!(Self::ARITY <= MAX_ARITY);
        assert!(Self::ARITY as usize + 1 == Self::NAMES.len());
        assert!(Self::NAMES.last().unwrap().is_null());
    };

    #[doc(hidden)]
    const ARITY: u8 = Self::NAMES.len() as u8 - 1;

    /// TODO: docs.
    const NAMES: &'static [*const c_char];

    /// TODO: docs.
    unsafe fn from_raw(
        args: *mut *mut sys::Value,
        ctx: &mut Context,
    ) -> Result<Self>;
}

trait TypeErasedPrimOpFun: 'static {
    unsafe fn call(
        &self,
        args: *mut *mut sys::Value,
        ret: *mut sys::Value,
        ctx: &mut Context,
    );
}

impl<P: PrimOpFun> TypeErasedPrimOpFun for P {
    unsafe fn call(
        &self,
        args: *mut *mut sys::Value,
        ret: *mut sys::Value,
        ctx: &mut Context,
    ) {
        let mut try_block = || unsafe {
            let args = <Self as PrimOpFun>::Args::from_raw(args, ctx)?;
            let val = PrimOpFun::call(self, args, ctx).try_into_value(ctx)?;
            val.write(ret, ctx)
        };

        // Errors are handled by setting the `Context::inner` field, so we
        // can ignore the result here.
        let _ = try_block();
    }
}
