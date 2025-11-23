//! TODO: docs.

use alloc::ffi::CString;
use core::ffi::{CStr, c_uint};
use core::marker::PhantomData;
use core::ptr::{self, NonNull};

use nix_bindings_sys as sys;

use crate::error::{Result, ToError, TryFromI64Error, TypeMismatchError};
use crate::namespace::Namespace;
use crate::prelude::{Context, PrimOp};

/// TODO: docs.
pub trait Value {
    /// TODO: docs.
    #[inline]
    fn borrow(&self) -> impl Value {
        struct BorrowedValue<'a, T: ?Sized> {
            inner: &'a T,
        }

        impl<T: Value + ?Sized> Value for BorrowedValue<'_, T> {
            #[inline]
            fn borrow(&self) -> impl Value {
                Self { inner: self.inner }
            }

            #[inline]
            fn kind(&self) -> ValueKind {
                self.inner.kind()
            }

            #[inline]
            unsafe fn write(
                &self,
                dest: NonNull<sys::Value>,
                ctx: &mut Context,
            ) -> Result<()> {
                unsafe { self.inner.write(dest, ctx) }
            }

            #[inline]
            unsafe fn write_with_namespace(
                &self,
                dest: NonNull<sys::Value>,
                namespace: impl Namespace,
                ctx: &mut Context,
            ) -> Result<()> {
                unsafe {
                    self.inner.write_with_namespace(dest, namespace, ctx)
                }
            }
        }

        BorrowedValue { inner: self }
    }

    /// Returns the kind of this value.
    fn kind(&self) -> ValueKind;

    /// Writes this value into the given, pre-allocated destination.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the `dest` points to a value that has been
    /// allocated but *not* yet initialized.
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

/// A trait for types that can be fallibly converted from a [`sys::Value`]
/// pointer.
pub trait TryFromValue<'a>: Sized + 'a {
    /// TODO: docs.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `value` is a valid pointer to a
    /// `sys::Value`.
    unsafe fn try_from_value(
        value: ValuePointer<'a>,
        ctx: &mut Context,
    ) -> Result<Self>;
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
    const LEN: c_uint;

    /// TODO: docs.
    fn with_value<T>(&self, value_idx: c_uint, fun: impl FnOnceValue<T>) -> T;
}

/// A trait to get around the lack of support for generics in closures.
///
/// This is semantically equivalent to `FnOnce<V: Value>(V) -> T`.
pub trait FnOnceValue<T> {
    /// Calls the function with the given value.
    fn call(self, value: impl Value) -> T;
}

/// TODO: docs.
#[derive(Debug, Copy, Clone)]
pub struct ValuePointer<'a> {
    ptr: NonNull<sys::Value>,
    _lifetime: PhantomData<&'a sys::Value>,
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

impl ValuePointer<'_> {
    /// TODO: docs.
    #[inline]
    pub fn as_ptr(self) -> NonNull<sys::Value> {
        self.ptr
    }

    /// TODO: docs.
    #[inline]
    pub fn as_raw(self) -> *mut sys::Value {
        self.ptr.as_ptr()
    }

    /// # Safety
    ///
    /// The caller must ensure that the value has been initialized.
    #[inline]
    pub(crate) unsafe fn new(inner: NonNull<sys::Value>) -> Self {
        Self { ptr: inner, _lifetime: PhantomData }
    }
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

#[cfg(feature = "std")]
impl Value for std::path::Path {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::Path
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        ctx: &mut Context,
    ) -> Result<()> {
        let bytes = self.as_os_str().as_encoded_bytes();
        let cstr = CString::new(bytes).map_err(|err| ctx.make_error(err))?;
        ctx.with_raw_and_state(|ctx, state| unsafe {
            sys::init_path_string(
                ctx,
                state.as_ptr(),
                dest.as_ptr(),
                cstr.as_ptr(),
            );
        })
    }
}

#[cfg(feature = "std")]
impl Value for &std::path::Path {
    #[inline(always)]
    fn kind(&self) -> ValueKind {
        (*self).kind()
    }

    #[inline(always)]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { (*self).write(dest, ctx) }
    }
}

#[cfg(feature = "std")]
impl Value for std::path::PathBuf {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::Path
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { self.as_path().write(dest, ctx) }
    }
}

impl Value for ValuePointer<'_> {
    #[inline]
    fn kind(&self) -> ValueKind {
        // 'nix_get_type' errors when the value pointer is null or when the
        // value is not initizialized, but having a ValuePointer guarantees
        // neither of those can happen, so we can use a null context.
        let r#type = unsafe { sys::get_type(ptr::null_mut(), self.as_raw()) };

        match r#type {
            sys::ValueType_NIX_TYPE_ATTRS => ValueKind::Attrset,
            sys::ValueType_NIX_TYPE_BOOL => ValueKind::Bool,
            sys::ValueType_NIX_TYPE_EXTERNAL => ValueKind::External,
            sys::ValueType_NIX_TYPE_FLOAT => ValueKind::Float,
            sys::ValueType_NIX_TYPE_FUNCTION => ValueKind::Function,
            sys::ValueType_NIX_TYPE_INT => ValueKind::Int,
            sys::ValueType_NIX_TYPE_LIST => ValueKind::List,
            sys::ValueType_NIX_TYPE_NULL => ValueKind::Null,
            sys::ValueType_NIX_TYPE_PATH => ValueKind::Path,
            sys::ValueType_NIX_TYPE_STRING => ValueKind::String,
            sys::ValueType_NIX_TYPE_THUNK => ValueKind::Thunk,
            other => unreachable!("invalid ValueType: {other}"),
        }
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        _: &mut Context,
    ) -> Result<()> {
        // 'nix_copy_value' errors when:
        //
        // 1. the destination pointer is null;
        // 2. the destination value is already initialized;
        // 3. the source pointer is null;
        // 4. the source value is not initialized.
        //
        // Having a ValuePointer guarantees that (3) and (4) cannot happen,
        // having a NonNull destination pointer guarantees that (1) cannot
        // happen, and the API contract for this method guarantees that (2)
        // cannot happen, so we can use a null context.
        unsafe {
            sys::copy_value(ptr::null_mut(), dest.as_ptr(), self.as_raw());
        };
        Ok(())
    }
}

impl TryFromValue<'_> for i64 {
    #[inline]
    unsafe fn try_from_value(
        value: ValuePointer<'_>,
        ctx: &mut Context,
    ) -> Result<Self> {
        ctx.force(value.as_ptr())?;

        match value.kind() {
            ValueKind::Int => ctx
                .with_raw(|ctx| unsafe { sys::get_int(ctx, value.as_raw()) }),
            other => Err(ctx.make_error(TypeMismatchError {
                expected: ValueKind::Int,
                found: other,
            })),
        }
    }
}

macro_rules! impl_try_from_value_for_int {
    ($ty:ty) => {
        impl TryFromValue<'_> for $ty {
            #[inline]
            unsafe fn try_from_value(
                value: ValuePointer<'_>,
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

impl_try_from_value_for_int!(i8);
impl_try_from_value_for_int!(i16);
impl_try_from_value_for_int!(i32);
impl_try_from_value_for_int!(i128);
impl_try_from_value_for_int!(isize);

impl_try_from_value_for_int!(u8);
impl_try_from_value_for_int!(u16);
impl_try_from_value_for_int!(u32);
impl_try_from_value_for_int!(u64);
impl_try_from_value_for_int!(u128);
impl_try_from_value_for_int!(usize);

#[cfg(all(unix, feature = "std"))]
impl<'a> TryFromValue<'a> for &'a std::path::Path {
    #[inline]
    unsafe fn try_from_value(
        value: ValuePointer<'a>,
        ctx: &mut Context,
    ) -> Result<Self> {
        use std::os::unix::ffi::OsStrExt;

        ctx.force(value.as_ptr())?;

        match value.kind() {
            ValueKind::Path => {},
            other => {
                return Err(ctx.make_error(TypeMismatchError {
                    expected: ValueKind::Path,
                    found: other,
                }));
            },
        }

        let cstr_ptr = ctx.with_raw(|ctx| unsafe {
            sys::get_path_string(ctx, value.as_raw())
        })?;

        // SAFETY: the [docs] guarantee that the returned pointer is
        // valid for as long as the value is alive.
        //
        // [docs]: https://hydra.nixos.org/build/313564006/download/1/html/group__value__extract.html#ga3420055c22accfd07cc5537210d748a9
        let cstr = unsafe { CStr::from_ptr(cstr_ptr) };

        let os_str = std::ffi::OsStr::from_bytes(cstr.to_bytes());

        Ok(std::path::Path::new(os_str))
    }
}

#[cfg(feature = "std")]
impl TryFromValue<'_> for std::path::PathBuf {
    #[inline]
    unsafe fn try_from_value(
        value: ValuePointer<'_>,
        ctx: &mut Context,
    ) -> Result<Self> {
        unsafe { <&std::path::Path>::try_from_value(value, ctx) }
            .map(|path| path.to_owned())
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
                const LEN: c_uint = count!($($K)*);

                #[track_caller]
                #[inline]
                fn with_value<T>(
                    &self,
                    value_idx: c_uint,
                    _fun: impl FnOnceValue<T>,
                ) -> T {
                    match value_idx {
                        $($idx => _fun.call(self.$idx.borrow()),)*
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
    fn panic_tuple_index_oob(idx: c_uint, len: c_uint) -> ! {
        panic!("{len}-tuple received out of bounds index: {idx}")
    }
}
