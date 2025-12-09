//! TODO: docs.

use core::ffi::c_uint;
use core::ptr;
use core::ptr::NonNull;

use {nix_bindings_cpp as cpp, nix_bindings_sys as sys};

use crate::attrset::NixAttrset;
use crate::context::Context;
use crate::error::{Result, TypeMismatchError};
use crate::namespace::Namespace;
use crate::thunk::Thunk;
use crate::value::{
    FnOnceValue,
    IntoValue,
    NixValue,
    TryFromValue,
    Value,
    ValueKind,
    Values,
};

/// TODO: docs.
pub trait Callable {
    /// TODO: docs.
    fn value(&self) -> NixValue<'_>;

    /// TODO: docs.
    #[inline]
    fn call(
        &self,
        arg: impl IntoValue,
        ctx: &mut Context,
    ) -> Result<Thunk<'static>> {
        let dest_ptr = ctx.alloc_value()?;
        let arg_ptr = ctx.alloc_value()?;

        let res = (unsafe { arg.into_value().write_no_primop(arg_ptr, ctx) })
            .and_then(|()| {
                ctx.with_raw(|ctx| {
                    unsafe {
                        sys::init_apply(
                            ctx,
                            dest_ptr.as_ptr(),
                            self.value().as_raw(),
                            arg_ptr.as_ptr(),
                        )
                    };
                })
            });

        // Free the argument once we're done with it.
        ctx.with_raw(|ctx| unsafe { sys::value_decref(ctx, arg_ptr.as_ptr()) })
            .ok();

        // Free the destination value if the call failed.
        if let Err(err) = res {
            ctx.with_raw(|ctx| unsafe {
                sys::value_decref(ctx, dest_ptr.as_ptr())
            })
            .ok();
            return Err(err);
        }

        // SAFETY: `init_apply` has initialized the value at `dest_ptr`.
        let value = unsafe { NixValue::new(dest_ptr) };

        Ok(Thunk::new(value))
    }

    /// TODO: docs.
    ///
    /// # Panics
    ///
    /// Panics if the number of arguments is less than 2.
    #[inline]
    #[track_caller]
    #[allow(clippy::too_many_lines)]
    fn call_multi(
        &self,
        args: impl Values,
        ctx: &mut Context,
    ) -> Result<Thunk<'static>> {
        const fn values_len<V: Values>(_: &V) -> c_uint {
            V::LEN
        }

        fn values_array<V: Values>(_: &V) -> impl AsMut<[*mut sys::Value]> {
            V::array(|_| ptr::null_mut())
        }

        let args_len = values_len(&args);

        assert!(
            args_len >= 2,
            "Callable::call_multi() requires at least 2 arguments"
        );

        let dest_ptr = ctx.alloc_value()?;

        let mut args_array = values_array(&args);

        // We'll do an eager call with the first N - 1 arguments, followed by
        // a lazy call with the last argument.
        let args_slice = &mut args_array.as_mut()[..args_len as usize - 1];

        let mut num_written = 0;

        let mut try_write_args = || {
            struct WriteArg<'ctx, 'eval> {
                dest: NonNull<sys::Value>,
                ctx: &'ctx mut Context<'eval>,
            }
            impl FnOnceValue<Result<()>> for WriteArg<'_, '_> {
                #[inline]
                fn call(self, value: impl Value, _: ()) -> Result<()> {
                    unsafe { value.write_no_primop(self.dest, self.ctx) }
                }
            }
            for (arg_idx, arg_ptr) in args_slice.iter_mut().enumerate() {
                let dest = ctx.alloc_value()?;
                args.with_value(arg_idx as c_uint, WriteArg { dest, ctx })?;
                *arg_ptr = dest.as_ptr();
                num_written += 1;
            }
            Result::Ok(())
        };

        let res = try_write_args().and_then(|()| {
            ctx.with_raw_and_state(|ctx, state| unsafe {
                cpp::value_call_multi(
                    ctx,
                    state.as_ptr(),
                    self.value().as_raw(),
                    args_slice.len(),
                    args_slice.as_mut_ptr(),
                    dest_ptr.as_ptr(),
                );
            })
        });

        // Free the arguments once we're done with them.
        for &raw_arg in &args_slice[..num_written] {
            ctx.with_raw(|ctx| unsafe { sys::value_decref(ctx, raw_arg) }).ok();
        }

        // Free the destination value if the call failed.
        if let Err(err) = res {
            ctx.with_raw(|ctx| unsafe {
                sys::value_decref(ctx, dest_ptr.as_ptr())
            })
            .ok();
            return Err(err);
        }

        // SAFETY: `value_call_multi` has initialized the value at `dest_ptr`.
        let value = unsafe { NixValue::new(dest_ptr) };

        let lambda = NixLambda::try_from_value(value, ctx)?;

        struct LazyCallLastArg<'lambda, 'ctx, 'eval> {
            lambda: NixLambda<'lambda>,
            ctx: &'ctx mut Context<'eval>,
        }

        impl FnOnceValue<Result<Thunk<'static>>> for LazyCallLastArg<'_, '_, '_> {
            #[inline]
            fn call(self, value: impl Value, _: ()) -> Result<Thunk<'static>> {
                self.lambda.call(value, self.ctx)
            }
        }

        args.with_value(args_len - 1 as c_uint, LazyCallLastArg { lambda, ctx })
    }
}

/// TODO: docs.
#[derive(Copy, Clone)]
pub struct NixFunctor<'value> {
    inner: NixAttrset<'value>,
}

/// TODO: docs.
#[derive(Copy, Clone)]
pub struct NixLambda<'value> {
    inner: NixValue<'value>,
}

impl Callable for NixFunctor<'_> {
    #[inline]
    fn value(&self) -> NixValue<'_> {
        self.inner.into()
    }
}

impl<'a> TryFromValue<NixValue<'a>> for NixFunctor<'a> {
    #[inline]
    fn try_from_value(value: NixValue<'a>, ctx: &mut Context) -> Result<Self> {
        NixAttrset::try_from_value(value, ctx)
            .and_then(|attrset| Self::try_from_value(attrset, ctx))
    }
}

impl<'a> TryFromValue<NixAttrset<'a>> for NixFunctor<'a> {
    #[inline]
    fn try_from_value(
        attrset: NixAttrset<'a>,
        ctx: &mut Context,
    ) -> Result<Self> {
        match attrset.get::<NixValue>(c"__functor", ctx)?.kind() {
            // We also accept thunks to avoid eagerly forcing functors. If the
            // __functor doesn't evaluates to a function, the user will get an
            // error when calling 'Callable::call{_multi}()'.
            ValueKind::Function | ValueKind::Thunk => {
                Ok(Self { inner: attrset })
            },
            other => Err(TypeMismatchError {
                expected: ValueKind::Function,
                found: other,
            }.into()),
        }
    }
}

impl Value for NixFunctor<'_> {
    #[inline]
    fn kind(&self) -> ValueKind {
        self.inner.kind()
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { self.inner.write(dest, namespace, ctx) }
    }
}

impl Callable for NixLambda<'_> {
    #[inline]
    fn value(&self) -> NixValue<'_> {
        self.inner
    }
}

impl<'a> TryFromValue<NixValue<'a>> for NixLambda<'a> {
    #[inline]
    fn try_from_value(
        mut value: NixValue<'a>,
        ctx: &mut Context,
    ) -> Result<Self> {
        value.force_inline(ctx)?;

        match value.kind() {
            ValueKind::Function => Ok(Self { inner: value }),
            other => Err(TypeMismatchError {
                expected: ValueKind::Function,
                found: other,
            }.into()),
        }
    }
}

impl Value for NixLambda<'_> {
    #[inline]
    fn kind(&self) -> ValueKind {
        self.inner.kind()
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { self.inner.write(dest, namespace, ctx) }
    }
}
