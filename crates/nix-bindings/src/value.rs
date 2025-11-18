use core::ffi::CStr;
use std::ffi::CString;

use nix_bindings_sys as sys;

use crate::{Attrset, Context, PrimOp, Result, ToError};

/// TODO: docs.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ValueKind {
    /// TODO: docs.
    Attrset,

    /// TODO: docs.
    Bool,

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
    String,

    /// TODO: docs.
    Thunk,
}

/// TODO: docs.
pub trait Value: Sealed + Sized {
    /// Returns the kind of this value.
    fn kind(&self) -> ValueKind;

    /// Writes this value into the given, pre-allocated destination.
    #[doc(hidden)]
    unsafe fn write(
        self,
        dest: *mut sys::Value,
        ctx: &mut Context,
    ) -> Result<()>;
}

/// A trait for types that can be fallibly converted into [`Value`]s.
pub trait TryIntoValue {
    /// Attempts to convert this value into a [`Value`].
    fn try_into_value(
        self,
        ctx: &mut Context,
    ) -> Result<impl Value + use<Self>>;
}

impl Value for () {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::Null
    }

    #[inline]
    unsafe fn write(
        self,
        dest: *mut sys::Value,
        ctx: &mut Context,
    ) -> Result<()> {
        ctx.with_inner_raw(|ctx| unsafe {
            sys::init_null(ctx, dest);
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
        self,
        dest: *mut sys::Value,
        ctx: &mut Context,
    ) -> Result<()> {
        ctx.with_inner_raw(|ctx| unsafe {
            sys::init_bool(ctx, dest, self);
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
                self,
                dest: *mut sys::Value,
                ctx: &mut Context,
            ) -> Result<()> {
                ctx.with_inner_raw(|ctx| unsafe {
                    sys::init_int(ctx, dest, self.into());
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
                self,
                dest: *mut sys::Value,
                ctx: &mut Context,
            ) -> Result<()> {
                ctx.with_inner_raw(|ctx| unsafe {
                    sys::init_float(ctx, dest, self.into());
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
        self,
        dest: *mut sys::Value,
        ctx: &mut Context,
    ) -> Result<()> {
        ctx.with_inner_raw(|ctx| unsafe {
            sys::init_string(ctx, dest, self.as_ptr());
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
        self,
        dest: *mut sys::Value,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { (&*self).write(dest, ctx) }
    }
}

impl Value for &str {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::String
    }

    #[inline]
    unsafe fn write(
        self,
        dest: *mut sys::Value,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { self.to_owned().write(dest, ctx) }
    }
}

impl Value for String {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::String
    }

    #[inline]
    unsafe fn write(
        self,
        dest: *mut sys::Value,
        ctx: &mut Context,
    ) -> Result<()> {
        let string = CString::new(self).map_err(|err| ctx.make_error(err))?;
        unsafe { string.write(dest, ctx) }
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
        self,
        dest: *mut sys::Value,
        ctx: &mut Context,
    ) -> Result<()> {
        match self {
            Some(value) => unsafe { value.write(dest, ctx) },
            None => unsafe { ().write(dest, ctx) },
        }
    }
}

impl<P: PrimOp> Value for P {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::Function
    }

    #[inline]
    unsafe fn write(
        self,
        dest: *mut sys::Value,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe {
            let primop_ptr = ctx.with_inner(|ctx| self.alloc(ctx))?;
            ctx.with_inner_raw(|ctx| sys::init_primop(ctx, dest, primop_ptr))?;
            ctx.with_inner_raw(|ctx| sys::gc_decref(ctx, primop_ptr.cast()))?;
            Ok(())
        }
    }
}

/// A newtype wrapper that implements `Value` for every `Attrset`.
pub(crate) struct AttrsetValue<T>(pub(crate) T);

impl<T: Attrset> Value for AttrsetValue<T> {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::Attrset
    }

    #[inline]
    unsafe fn write(
        self,
        _dest: *mut sys::Value,
        _ctx: &mut Context,
    ) -> Result<()> {
        let Self(_attrset) = self;
        todo!();
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

use sealed::Sealed;

mod sealed {
    use super::*;

    pub trait Sealed {}

    impl Sealed for () {}

    impl Sealed for bool {}

    impl Sealed for u8 {}
    impl Sealed for u16 {}
    impl Sealed for u32 {}
    impl Sealed for i8 {}
    impl Sealed for i16 {}
    impl Sealed for i32 {}
    impl Sealed for i64 {}

    impl Sealed for f32 {}
    impl Sealed for f64 {}

    impl Sealed for &CStr {}
    impl Sealed for CString {}

    impl Sealed for &str {}
    impl Sealed for String {}

    impl<T: Value> Sealed for Option<T> {}

    impl<P: PrimOp> Sealed for P {}

    impl<T> Sealed for AttrsetValue<T> {}
}
