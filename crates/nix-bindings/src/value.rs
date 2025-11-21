//! TODO: docs.

use alloc::ffi::CString;
use core::ffi::CStr;
use core::ptr::NonNull;

use nix_bindings_sys as sys;

use crate::namespace::Namespace;
use crate::prelude::{Context, PrimOp, Result, ToError};

/// TODO: docs.
pub trait Value: Sized {
    /// Returns the kind of this value.
    fn kind(&self) -> ValueKind;

    /// Writes this value into the given, pre-allocated destination.
    #[doc(hidden)]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        ctx: &mut Context,
    ) -> Result<()>;

    /// TODO: docs.
    #[doc(hidden)]
    #[inline]
    unsafe fn write_with_namespace(
        &self,
        dest: NonNull<sys::Value>,
        #[expect(unused_variables)] namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        debug_assert_ne!(self.kind(), ValueKind::Attrset);
        unsafe { self.write(dest, ctx) }
    }
}

/// A trait for types that can be fallibly converted into [`Value`]s.
pub trait TryIntoValue {
    /// Attempts to convert this value into a [`Value`].
    fn try_into_value(
        self,
        ctx: &mut Context,
    ) -> Result<impl Value + use<Self>>;
}

/// TODO: docs.
pub trait Values {
    /// TODO: docs.
    const LEN: usize;

    /// TODO: docs.
    fn with_value<'a, T: 'a>(
        &'a self,
        value_idx: usize,
        fun: impl FnOnceValue<'a, T>,
    ) -> T;
}

/// A trait to get around the lack of support for generics in closures.
///
/// This is semantically equivalent to `FnOnce(&impl Value) -> T`.
pub trait FnOnceValue<'a, T: 'a> {
    /// Calls the function with the given value.
    fn call(self, value: &'a impl Value) -> T;
}

/// TODO: docs.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ValueKind {
    /// TODO: docs.
    Attrset,

    /// TODO: docs.
    Bool,

    /// TODO: docs.
    External,

    /// TODO: docs.
    Float,

    /// TODO: docs.
    Function,

    /// TODO: docs.
    Int,

    /// TODO: docs.
    List,

    /// TODO: docs.
    Null,

    /// TODO: docs.
    Path,

    /// TODO: docs.
    String,

    /// TODO: docs.
    Thunk,
}

impl Value for () {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::Null
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        ctx: &mut Context,
    ) -> Result<()> {
        ctx.with_raw(|ctx| unsafe {
            sys::init_null(ctx, dest.as_ptr());
        })
    }
}

impl Value for bool {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::Bool
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        ctx: &mut Context,
    ) -> Result<()> {
        ctx.with_raw(|ctx| unsafe {
            sys::init_bool(ctx, dest.as_ptr(), *self);
        })
    }
}

macro_rules! impl_value_for_int {
    ($ty:ty) => {
        impl Value for $ty {
            #[inline]
            fn kind(&self) -> ValueKind {
                ValueKind::Int
            }

            #[inline]
            unsafe fn write(
                &self,
                dest: NonNull<sys::Value>,
                ctx: &mut Context,
            ) -> Result<()> {
                ctx.with_raw(|ctx| unsafe {
                    sys::init_int(ctx, dest.as_ptr(), (*self).into());
                })
            }
        }
    };
}

impl_value_for_int!(u8);
impl_value_for_int!(u16);
impl_value_for_int!(u32);
impl_value_for_int!(i8);
impl_value_for_int!(i16);
impl_value_for_int!(i32);
impl_value_for_int!(i64);

macro_rules! impl_value_for_float {
    ($ty:ty) => {
        impl Value for $ty {
            #[inline]
            fn kind(&self) -> ValueKind {
                ValueKind::Float
            }

            #[inline]
            unsafe fn write(
                &self,
                dest: NonNull<sys::Value>,
                ctx: &mut Context,
            ) -> Result<()> {
                ctx.with_raw(|ctx| unsafe {
                    sys::init_float(ctx, dest.as_ptr(), (*self).into());
                })
            }
        }
    };
}

impl_value_for_float!(f32);
impl_value_for_float!(f64);

impl Value for &CStr {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::String
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        ctx: &mut Context,
    ) -> Result<()> {
        ctx.with_raw(|ctx| unsafe {
            sys::init_string(ctx, dest.as_ptr(), self.as_ptr());
        })
    }
}

impl Value for CString {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::String
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { self.as_c_str().write(dest, ctx) }
    }
}

impl Value for &str {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::String
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        ctx: &mut Context,
    ) -> Result<()> {
        let string = CString::new(*self).map_err(|err| ctx.make_error(err))?;
        unsafe { string.write(dest, ctx) }
    }
}

impl Value for alloc::string::String {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::String
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { self.as_str().write(dest, ctx) }
    }
}

impl<T: Value> Value for Option<T> {
    #[inline]
    fn kind(&self) -> ValueKind {
        match self {
            Some(value) => value.kind(),
            None => ValueKind::Null,
        }
    }

    #[inline]
    unsafe fn write(
        &self,
        _: NonNull<sys::Value>,
        _: &mut Context,
    ) -> Result<()> {
        unreachable!()
    }

    #[inline]
    unsafe fn write_with_namespace(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        match self {
            Some(value) => unsafe {
                value.write_with_namespace(dest, namespace, ctx)
            },
            None => unsafe { ().write(dest, ctx) },
        }
    }
}

impl<P: PrimOp> Value for P {
    #[inline]
    fn kind(&self) -> ValueKind {
        // FIXME: this is not always correct.
        ValueKind::Function
    }

    #[inline]
    unsafe fn write(
        &self,
        _: NonNull<sys::Value>,
        _: &mut Context,
    ) -> Result<()> {
        unreachable!()
    }

    #[inline]
    unsafe fn write_with_namespace(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        ctx.write_primop::<P>(namespace, dest)
    }
}

impl<T: Value> TryIntoValue for T {
    #[inline]
    fn try_into_value(self, _: &mut Context) -> Result<impl Value + use<T>> {
        Ok(self)
    }
}

impl<T: TryIntoValue> TryIntoValue for Result<T> {
    #[inline]
    fn try_into_value(self, ctx: &mut Context) -> Result<impl Value + use<T>> {
        match self {
            Ok(value) => value.try_into_value(ctx),
            Err(err) => Err(err),
        }
    }
}

impl<T: TryIntoValue, E: ToError> TryIntoValue for core::result::Result<T, E> {
    #[inline]
    fn try_into_value(
        self,
        ctx: &mut Context,
    ) -> Result<impl Value + use<T, E>> {
        match self {
            Ok(value) => value.try_into_value(ctx),
            Err(err) => Err(ctx.make_error(err)),
        }
    }
}

#[rustfmt::skip]
mod values_impls {
    use super::*;

    macro_rules! count {
        () => { 0 };
        ($x:tt $($xs:tt)*) => { 1 + count!($($xs)*) };
    }

    macro_rules! impl_values {
        ($($K:ident),*) => {
            impl_values!(@pair [] [0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31] [$($K)*]);
        };

        (@pair [$($pairs:tt)*] [$next_idx:tt $($rest_idx:tt)*] [$next_K:ident $($rest_K:ident)*]) => {
            impl_values!(@pair [$($pairs)* ($next_idx $next_K)] [$($rest_idx)*] [$($rest_K)*]);
        };

        (@pair [$(($idx:tt $K:ident))*] $_:tt []) => {
            impl<$($K),*> Values for ($($K,)*)
            where
                $($K: Value),*
            {
                const LEN: usize = count!($($K)*);

                #[track_caller]
                #[inline]
                fn with_value<'a, T: 'a>(
                    &'a self,
                    value_idx: usize,
                    _fun: impl FnOnceValue<'a, T>,
                ) -> T {
                    match value_idx {
                        $($idx => _fun.call(&self.$idx),)*
                        other => panic_tuple_index_oob(other, <Self as Values>::LEN),
                    }
                }
            }
        };
    }

    impl_values!();
    impl_values!(V);
    impl_values!(V1, V2);
    impl_values!(V1, V2, V3);
    impl_values!(V1, V2, V3, V4);
    impl_values!(V1, V2, V3, V4, V5);
    impl_values!(V1, V2, V3, V4, V5, V6);
    impl_values!(V1, V2, V3, V4, V5, V6, V7);
    impl_values!(V1, V2, V3, V4, V5, V6, V7, V8);
    impl_values!(V1, V2, V3, V4, V5, V6, V7, V8, V9);
    impl_values!(V1, V2, V3, V4, V5, V6, V7, V8, V9, V10);
    impl_values!(V1, V2, V3, V4, V5, V6, V7, V8, V9, V10, V11);
    impl_values!(V1, V2, V3, V4, V5, V6, V7, V8, V9, V10, V11, V12);
    impl_values!(V1, V2, V3, V4, V5, V6, V7, V8, V9, V10, V11, V12, V13);
    impl_values!(V1, V2, V3, V4, V5, V6, V7, V8, V9, V10, V11, V12, V13, V14);
    impl_values!(V1, V2, V3, V4, V5, V6, V7, V8, V9, V10, V11, V12, V13, V14, V15);
    impl_values!(V1, V2, V3, V4, V5, V6, V7, V8, V9, V10, V11, V12, V13, V14, V15, V16);

    #[inline(never)]
    fn panic_tuple_index_oob(idx: usize, len: usize) -> ! {
        panic!("{len}-tuple received out of bounds index: {idx}")
    }
}
