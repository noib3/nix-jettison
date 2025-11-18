use core::ffi::CStr;
use std::ffi::CString;

use nix_bindings_sys as sys;

use crate::{PrimOp, Result};

/// TODO: docs.
pub trait Value: Sealed + Sized {
    /// Writes this value into the given, pre-allocated destination.
    #[doc(hidden)]
    unsafe fn write(
        self,
        dest: *mut sys::Value,
        ctx: &mut Context,
    ) -> Result<()>;
}

impl Value for () {
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

use sealed::Sealed;

use crate::Context;

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
}
