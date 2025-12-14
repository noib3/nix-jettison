//! TODO: docs.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use core::ffi::{CStr, c_char, c_void};
use core::marker::PhantomData;
use core::ptr::NonNull;

use nix_bindings_sys as sys;

use crate::Utf8CStr;
use crate::context::{Context, EvalState};
use crate::error::{Error, ErrorKind, Result};
use crate::value::{NixValue, TryFromValue, TryIntoValue, Value, ValueKind};

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
        let primop = Self::NEW;

        unsafe {
            sys::alloc_primop(
                ctx.as_ptr(),
                primop.c_fun(),
                primop.arity().into(),
                namespace.as_ptr(),
                primop.arg_names().as_ptr().cast_mut(),
                Self::DOCS.as_ptr(),
                // This is a leak, but it's ok because it only happens once in
                // the lifetime of the plugin.
                Box::into_raw(Box::new(UserData { primop, namespace })).cast(),
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
                    "received NULL `Value` pointer for return value in primop \
                     call"
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

    #[doc(hidden)]
    fn value_kind(&self) -> ValueKind;
}

/// TODO: docs.
pub trait Constant {
    /// TODO: docs.
    fn value() -> impl Value;
}

/// TODO: docs.
pub trait Function {
    /// TODO: docs.
    type Args<'a>: Args<'a>;

    /// TODO: docs.
    fn call<'a: 'a>(
        args: Self::Args<'a>,
        ctx: &mut Context,
    ) -> impl TryIntoValue + use<'a, Self>;

    #[doc(hidden)]
    #[inline(always)]
    fn value_kind() -> ValueKind {
        ValueKind::Function
    }
}

/// TODO: docs.
pub trait Args<'a>: Sized {
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
        args_list: ArgsList<'a>,
        ctx: &mut Context,
    ) -> Result<Self>;
}

/// TODO: docs.
#[derive(Debug, Copy, Clone)]
pub struct ArgsList<'a> {
    inner: NonNull<*mut sys::Value>,
    _lifetime: PhantomData<&'a [*mut sys::Value]>,
}

/// The user data given as the last argument to [`sys::alloc_primop`].
struct UserData {
    /// The type-erased primop.
    primop: &'static dyn PrimOpImpl,

    /// The namespace given to `PrimOp::alloc`.
    namespace: Cow<'static, CStr>,
}

impl<'a> ArgsList<'a> {
    /// Gets the argument at the given offset and tries to convert it to
    /// the desired type.
    ///
    /// Returns an error if the pointer at the given offset is NULL or if the
    /// conversion fails.
    ///
    /// This is only meant to be used in the code generated by the
    /// [`Args`](crate::Args) derive macro, and is not part of this type's
    /// public API.
    #[doc(hidden)]
    #[inline]
    pub unsafe fn get<T: TryFromValue<NixValue<'a>>>(
        self,
        offset: u8,
        ctx: &mut Context,
    ) -> Result<T> {
        let arg_raw = unsafe { *self.inner.as_ptr().offset(offset.into()) };
        let arg_ptr = NonNull::new(arg_raw).ok_or_else(|| {
            Error::new(ErrorKind::Overflow, c"argument is NULL")
        })?;
        // SAFETY: the argument list comes from a primop callback, so every
        // argument has been initialized.
        unsafe { T::try_from_value(NixValue::new(arg_ptr), ctx) }
    }
}

impl<C: Constant> Function for C {
    type Args<'a> = NoArgs;

    #[inline]
    fn call<'a: 'a>(_: NoArgs, _: &mut Context) -> impl Value + use<C> {
        C::value()
    }

    #[inline(always)]
    fn value_kind() -> ValueKind {
        C::value().kind()
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
        args: NonNull<*mut sys::Value>,
        ret: NonNull<sys::Value>,
        namespace: &Cow<'static, CStr>,
        ctx: &mut Context,
    ) {
        #[allow(path_statements)]
        F::Args::CHECKS;

        let args_list = ArgsList { inner: args, _lifetime: PhantomData };

        let mut try_block = || unsafe {
            let args = <Self as Function>::Args::from_raw(args_list, ctx)?;
            let mut val = Self::call(args, ctx).try_into_value(ctx)?;
            // As described in the [docs] of `nix_init_apply`, it's not
            // possible to return thunks from primops, so let's force the value
            // before writing it to the return location.
            //
            // [docs]: https://github.com/NixOS/nix/blob/af0ac14/src/libexpr-c/nix_api_value.h#L564
            val.force_inline(ctx)?;
            val.write(ret, namespace, ctx)
        };

        if let Err(err) = try_block() {
            unsafe {
                sys::set_err_msg(
                    ctx.inner_mut().as_raw(),
                    err.kind().code(),
                    err.message().as_ptr(),
                );
            }
        }
    }

    #[inline(always)]
    fn value_kind(&self) -> ValueKind {
        F::value_kind()
    }
}

#[doc(hidden)]
pub struct NoArgs;

impl Args<'_> for NoArgs {
    const NAMES: &'static [*const c_char] = &[core::ptr::null()];

    #[inline]
    unsafe fn from_raw(_: ArgsList<'_>, _: &mut Context) -> Result<Self> {
        Ok(Self)
    }
}
