//! TODO: docs.

use core::ffi::{CStr, c_char, c_void};
use core::ptr::NonNull;

use nix_bindings_sys as sys;

use crate::context::{Context, EvalState};
use crate::error::{Result, TryFromI64Error, TypeMismatchError};
use crate::value::{TryIntoValue, Value, ValueKind};

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

        let type_erased: Box<dyn DynCompatPrimOpFun> = Box::new(self);

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
            let primop: &dyn DynCompatPrimOpFun =
                unsafe { &**(user_data as *mut Box<dyn DynCompatPrimOpFun>) };

            let Some(args) = NonNull::new(args) else {
                panic!("received NULL args pointer in primop call");
            };

            let Some(ret) = NonNull::new(ret) else {
                panic!(
                    "received NULL `Value` pointer for return value in \
                     primop call"
                );
            };

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
        assert!(Self::ARITY <= sys::MAX_PRIMOP_ARITY);
        assert!(Self::ARITY as usize + 1 == Self::NAMES.len());
        assert!(Self::NAMES.last().unwrap().is_null());
    };

    /// TODO: docs.
    const ARITY: u8 = Self::NAMES.len() as u8 - 1;

    #[doc(hidden)]
    const NAMES: &'static [*const c_char];

    #[doc(hidden)]
    unsafe fn from_raw(
        args: NonNull<*mut sys::Value>,
        ctx: &mut Context,
    ) -> Result<Self>;
}

/// TODO: docs.
pub trait Arg: Sized {
    /// TODO: docs.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `value` is a valid pointer to a
    /// `sys::Value`.
    unsafe fn try_from_value(
        value: NonNull<sys::Value>,
        ctx: &mut Context,
    ) -> Result<Self>;
}

impl Args for () {
    const NAMES: &'static [*const c_char] = &[core::ptr::null()];

    #[inline]
    unsafe fn from_raw(
        args: NonNull<*mut nix_bindings_sys::Value>,
        _: &mut Context,
    ) -> Result<Self> {
        debug_assert! { unsafe { *args.offset(0).as_ptr() }.is_null() };
        Ok(())
    }
}

impl Arg for i64 {
    #[inline]
    unsafe fn try_from_value(
        value: NonNull<nix_bindings_sys::Value>,
        ctx: &mut Context,
    ) -> Result<Self> {
        ctx.value_force(value)?;

        match ctx.get_kind(value)? {
            ValueKind::Int => ctx.with_inner_raw(|ctx| unsafe {
                sys::get_int(ctx, value.as_ptr())
            }),
            other => Err(ctx.make_error(TypeMismatchError {
                expected: ValueKind::Int,
                found: other,
            })),
        }
    }
}

macro_rules! impl_arg_for_int {
    ($ty:ty) => {
        impl Arg for $ty {
            #[inline]
            unsafe fn try_from_value(
                value: NonNull<nix_bindings_sys::Value>,
                ctx: &mut Context,
            ) -> Result<Self> {
                let int = unsafe { i64::try_from_value(value, ctx)? };

                int.try_into().map_err(|_| {
                    ctx.make_error(TryFromI64Error::<$ty>::new(int))
                })
            }
        }
    };
}

impl_arg_for_int!(i8);
impl_arg_for_int!(i16);
impl_arg_for_int!(i32);
impl_arg_for_int!(i128);
impl_arg_for_int!(isize);

impl_arg_for_int!(u8);
impl_arg_for_int!(u16);
impl_arg_for_int!(u32);
impl_arg_for_int!(u64);
impl_arg_for_int!(u128);
impl_arg_for_int!(usize);

/// A dyn-compatible version of [`PrimOpFun`] that allows us to type-erase
/// [`PrimOp`]s in [`PrimOp::alloc`].
trait DynCompatPrimOpFun: 'static {
    unsafe fn call(
        &self,
        args: NonNull<*mut sys::Value>,
        ret: NonNull<sys::Value>,
        ctx: &mut Context,
    );
}

impl<P: PrimOpFun> DynCompatPrimOpFun for P {
    unsafe fn call(
        &self,
        args: NonNull<*mut sys::Value>,
        ret: NonNull<sys::Value>,
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
