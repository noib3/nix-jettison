//! TODO: docs.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use core::ffi::{CStr, c_char, c_void};
use core::ptr::NonNull;

use nix_bindings_sys as sys;

use crate::Utf8CStr;
use crate::context::{Context, EvalState};
use crate::error::{Result, TryFromI64Error, TypeMismatchError};
use crate::value::{TryIntoValue, Value, ValueKind};

/// TODO: docs.
pub trait PrimOp: PrimOpImpl + Sized + 'static {
    /// TODO: docs.
    const DOCS: &'static CStr;

    /// TODO: docs.
    const NAME: &'static Utf8CStr;

    /// TODO: docs.
    #[doc(hidden)]
    const NEW: &'static Self;

    #[doc(hidden)]
    #[inline]
    unsafe fn alloc(
        namespace: Cow<'static, CStr>,
        ctx: NonNull<sys::c_context>,
    ) -> *mut sys::PrimOp {
        let this = Self::NEW;

        unsafe {
            sys::alloc_primop(
                ctx.as_ptr(),
                this.c_fun(),
                this.arity().into(),
                namespace.as_ptr(),
                this.arg_names().as_ptr().cast_mut(),
                Self::DOCS.as_ptr(),
                // This is a leak, but it's ok because it only happens once in
                // the lifetime of the plugin.
                Box::into_raw(Box::new(UserData { primop: this, namespace }))
                    .cast(),
            )
        }
    }
}

/// TODO: docs.
pub trait PrimOpImpl {
    #[doc(hidden)]
    fn arg_names(&self) -> &'static [*const c_char];

    #[doc(hidden)]
    fn arity(&self) -> u8;

    #[allow(clippy::too_many_arguments)]
    #[allow(
        clippy::ptr_arg,
        reason = "&Cow<'static, CStr> implements Namespace, &CStr doesn't"
    )]
    #[doc(hidden)]
    unsafe fn call(
        &self,
        args: NonNull<*mut sys::Value>,
        ret: NonNull<sys::Value>,
        namespace: &Cow<'static, CStr>,
        ctx: &mut Context,
    );

    #[doc(hidden)]
    #[inline]
    fn c_fun(&self) -> sys::PrimOpFun {
        unsafe extern "C" fn wrapper(
            user_data: *mut c_void,
            ctx: *mut sys::c_context,
            state: *mut sys::EvalState,
            args: *mut *mut sys::Value,
            ret: *mut sys::Value,
        ) {
            // SAFETY:
            // - user_data was created in PrimOp::alloc from a UserData;
            // - the raw pointer is guaranteed to still point to valid memory
            //   because Box::from_raw is never called;
            // - UserData is 'static;
            let user_data = unsafe { &*(user_data as *mut UserData) };

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
                user_data.primop.call(
                    args,
                    ret,
                    &user_data.namespace,
                    &mut Context::new(ctx, EvalState::new(state)),
                );
            }
        }

        Some(wrapper)
    }
}

/// TODO: docs.
pub trait Constant {
    /// TODO: docs.
    fn value() -> impl Value;
}

/// TODO: docs.
pub trait Function {
    /// TODO: docs.
    type Args: Args;

    /// TODO: docs.
    fn call(
        args: Self::Args,
        ctx: &mut Context,
    ) -> impl TryIntoValue + use<Self>;
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

/// The user data given as the last argument to [`sys::alloc_primop`].
struct UserData {
    /// The type-erased primop.
    primop: &'static dyn PrimOpImpl,

    /// The namespace given to `PrimOp::alloc`.
    namespace: Cow<'static, CStr>,
}

impl<C: Constant> Function for C {
    type Args = NoArgs;

    #[inline]
    fn call(_: NoArgs, _: &mut Context) -> impl Value + use<C> {
        C::value()
    }
}

impl<F: Function> PrimOpImpl for F {
    #[inline(always)]
    fn arg_names(&self) -> &'static [*const c_char] {
        F::Args::NAMES
    }

    #[inline(always)]
    fn arity(&self) -> u8 {
        F::Args::ARITY
    }

    #[inline]
    unsafe fn call(
        &self,
        args: NonNull<*mut nix_bindings_sys::Value>,
        ret: NonNull<nix_bindings_sys::Value>,
        namespace: &Cow<'static, CStr>,
        ctx: &mut Context,
    ) {
        #[allow(path_statements)]
        F::Args::CHECKS;

        let mut try_block = || unsafe {
            let args = <Self as Function>::Args::from_raw(args, ctx)?;
            let val = Self::call(args, ctx).try_into_value(ctx)?;
            val.write_with_namespace(ret, namespace, ctx)
        };

        // Errors are handled by setting the `Context::inner` field, so we
        // can ignore the result here.
        let _ = try_block();
    }
}

#[doc(hidden)]
pub struct NoArgs;

impl Args for NoArgs {
    const NAMES: &'static [*const c_char] = &[core::ptr::null()];

    #[inline]
    unsafe fn from_raw(
        args: NonNull<*mut nix_bindings_sys::Value>,
        _: &mut Context,
    ) -> Result<Self> {
        debug_assert! { unsafe { *args.offset(0).as_ptr() }.is_null() };
        Ok(Self)
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
            ValueKind::Int => ctx
                .with_raw(|ctx| unsafe { sys::get_int(ctx, value.as_ptr()) }),
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
