use core::ffi::CStr;
use core::ptr::NonNull;
use std::ffi::CString;

use nix_bindings_sys as sys;

use crate::{Attrset, Context, LiteralAttrset, PrimOp, Result, ToError};

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
        &self,
        dest: NonNull<sys::Value>,
        ctx: &mut Context,
    ) -> Result<()> {
        ctx.with_inner_raw(|ctx| unsafe {
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
        ctx.with_inner_raw(|ctx| unsafe {
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
                ctx.with_inner_raw(|ctx| unsafe {
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
                ctx.with_inner_raw(|ctx| unsafe {
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
        ctx.with_inner_raw(|ctx| unsafe {
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

impl Value for String {
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
        dest: NonNull<sys::Value>,
        ctx: &mut Context,
    ) -> Result<()> {
        match self {
            Some(value) => unsafe { value.write(dest, ctx) },
            None => unsafe { ().write(dest, ctx) },
        }
    }
}

impl<P: PrimOp + Clone> Value for P {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::Function
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe {
            // TODO: alloc() is implemented by leaking, so calling this
            // repeatedly will cause memory leaks. Fix this.
            let primop_ptr = ctx.with_inner(|ctx| self.clone().alloc(ctx))?;
            ctx.with_inner_raw(|ctx| {
                sys::init_primop(ctx, dest.as_ptr(), primop_ptr)
            })?;
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
        &self,
        dest: NonNull<sys::Value>,
        ctx: &mut Context,
    ) -> Result<()> {
        let Self(attrset) = self;

        unsafe {
            let len = attrset.len();
            let mut builder = ctx.make_bindings_builder(len)?;
            for idx in 0..len {
                let key = attrset.get_key_as_c_str(idx);
                builder.insert(key, |dest, ctx| {
                    attrset.write_value(idx, dest, ctx)
                })?;
            }
            builder.build(dest)
        }
    }
}

impl<Keys, Values> Value for LiteralAttrset<Keys, Values>
where
    Self: Attrset,
{
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::Attrset
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<nix_bindings_sys::Value>,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { self.into_value().write(dest, ctx) }
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
