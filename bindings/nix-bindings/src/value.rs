//! TODO: docs.

use alloc::borrow::{Cow, ToOwned};
use alloc::ffi::CString;
use alloc::vec::Vec;
use core::ffi::{CStr, c_char, c_uint, c_void};
use core::marker::PhantomData;
use core::ops::Deref;
use core::ptr::{self, NonNull};
use core::slice;

use {nix_bindings_cpp as cpp, nix_bindings_sys as sys};

use crate::error::{
    Error,
    Result,
    TryFromI64Error,
    TryIntoI64Error,
    TypeMismatchError,
};
use crate::list::{List, NixList};
use crate::namespace::{Namespace, PoppableNamespace};
use crate::prelude::{Context, PrimOp};
use crate::primop::PrimOpImpl;

/// TODO: docs.
pub trait Value {
    /// Returns the kind of this value.
    fn kind(&self) -> ValueKind;

    /// Writes this value into the given, pre-allocated destination.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the `dest` points to a value that has been
    /// allocated but *not* yet initialized.
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()>;

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
                namespace: impl Namespace,
                ctx: &mut Context,
            ) -> Result<()> {
                unsafe { self.inner.write(dest, namespace, ctx) }
            }
        }

        BorrowedValue { inner: self }
    }

    /// TODO: docs.
    #[inline(always)]
    fn force_inline(&mut self, _ctx: &mut Context) -> Result<()> {
        Ok(())
    }

    /// TODO: docs.
    ///
    /// # Safety
    ///
    /// Panics if the call graph for [`Self::write`](Value::write) contains a
    /// call to [`PrimOp`]'s implementation of [`Value::write`](Value::write).
    ///
    /// # Safety
    ///
    /// Same as [`write`](Value::write).
    #[inline]
    unsafe fn write_no_primop(
        &self,
        dest: NonNull<sys::Value>,
        ctx: &mut Context,
    ) -> Result<()> {
        #[derive(Copy, Clone)]
        struct EmptyNamespace;

        impl Namespace for EmptyNamespace {
            #[inline(always)]
            fn push(self, _: &CStr) -> impl PoppableNamespace<Self> {
                self
            }
            #[track_caller]
            fn display(self) -> Cow<'static, CStr> {
                panic!(
                    "attempted to write a PrimOp within a call to \
                     Value::write_no_primop()"
                )
            }
        }

        impl PoppableNamespace<Self> for EmptyNamespace {
            #[inline(always)]
            fn pop(self) -> Self {
                self
            }
        }

        unsafe { self.write(dest, EmptyNamespace, ctx) }
    }
}

/// TODO: docs.
pub trait BoolValue: Value + Sized {
    /// # Safety
    ///
    /// This method should only be called after a successful call to
    /// [`kind`](Value::kind) returns [`ValueKind::Bool`].
    unsafe fn into_bool(self, ctx: &mut Context) -> Result<bool>;
}

/// TODO: docs.
pub trait IntValue: Value + Sized {
    /// # Safety
    ///
    /// This method should only be called after a successful call to
    /// [`kind`](Value::kind) returns [`ValueKind::Int`].
    unsafe fn into_int(self, ctx: &mut Context) -> Result<i64>;
}

/// TODO: docs.
pub trait StringValue: Value + Sized {
    /// TODO: docs.
    type String;

    /// # Safety
    ///
    /// This method should only be called after a successful call to
    /// [`kind`](Value::kind) returns [`ValueKind::String`].
    unsafe fn into_string(self, ctx: &mut Context) -> Result<Self::String>;
}

/// TODO: docs.
pub trait PathValue: Value + Sized {
    /// TODO: docs.
    type Path: AsRef<CStr>;

    /// # Safety
    ///
    /// This method should only be called after a successful call to
    /// [`kind`](Value::kind) returns [`ValueKind::Path`].
    unsafe fn into_path_string(self, ctx: &mut Context) -> Result<Self::Path>;
}

/// A trait for types that can be infallibly converted into [`Value`]s.
///
/// For fallible conversions, see [`TryIntoValue`].
pub trait IntoValue {
    /// Converts `self` into a [`Value`].
    fn into_value<'eval>(
        self,
        ctx: &mut Context<'eval>,
    ) -> impl Value + use<'eval, Self>;
}

/// A trait for types that can be infallibly converted into [`Value`]s by
/// reference.
///
/// For conversions from owned values, see [`IntoValue`].
pub trait ToValue {
    /// Converts `&self` into a [`Value`].
    fn to_value<'this, 'eval>(
        &'this self,
        ctx: &mut Context<'eval>,
    ) -> impl Value + use<'this, 'eval, Self>;

    /// TODO: docs.
    #[inline(always)]
    fn as_into_value(&self) -> impl IntoValue {
        struct RefIntoValue<'a, T: ?Sized> {
            inner: &'a T,
        }

        impl<'a, T: ToValue + ?Sized> IntoValue for RefIntoValue<'a, T> {
            #[inline]
            fn into_value<'eval>(
                self,
                ctx: &mut Context<'eval>,
            ) -> impl Value + use<'a, 'eval, T> {
                self.inner.to_value(ctx)
            }
        }

        RefIntoValue { inner: self }
    }
}

/// A trait for types that can be fallibly converted into [`Value`]s.
pub trait TryIntoValue {
    /// Attempts to convert this value into a [`Value`].
    fn try_into_value<'eval>(
        self,
        ctx: &mut Context<'eval>,
    ) -> Result<impl Value + use<'eval, Self>>;
}

/// A trait for types that can be fallibly converted from [`Value`]s.
pub trait TryFromValue<V: Value>: Sized {
    /// TODO: docs.
    fn try_from_value(value: V, ctx: &mut Context) -> Result<Self>;
}

/// TODO: docs.
pub trait Values {
    /// TODO: docs.
    const LEN: c_uint;

    /// TODO: docs.
    fn with_value<T>(&self, value_idx: c_uint, fun: impl FnOnceValue<T>) -> T;

    /// TODO: docs.
    #[inline]
    fn array<T>(constructor: impl FnMut(usize) -> T) -> impl AsMut<[T]> {
        #[cfg(nightly)]
        {
            core::array::from_fn::<_, { Self::LEN as usize }, _>(constructor)
        }
        #[cfg(not(nightly))]
        {
            (0..(Self::LEN as usize)).map(constructor).collect::<Vec<T>>()
        }
    }
}

/// A trait to get around the lack of support for generics in closures.
///
/// This is semantically equivalent to `FnOnce<V: Value>(V) -> T`.
pub trait FnOnceValue<T, Ctx = ()> {
    /// Calls the function with the given value.
    fn call(self, value: impl Value, ctx: Ctx) -> T;

    /// TODO: docs.
    #[inline]
    fn map_ctx<NewCtx>(
        self,
        map: impl FnOnce(NewCtx) -> Ctx,
    ) -> impl FnOnceValue<T, NewCtx>
    where
        Self: Sized,
    {
        struct Mapped<Inner, Map> {
            inner: Inner,
            map: Map,
        }

        impl<T, Inner, Map, OldCtx, NewCtx> FnOnceValue<T, NewCtx>
            for Mapped<Inner, Map>
        where
            Inner: FnOnceValue<T, OldCtx>,
            Map: FnOnce(NewCtx) -> OldCtx,
        {
            #[inline]
            fn call(self, value: impl Value, ctx: NewCtx) -> T {
                self.inner.call(value, (self.map)(ctx))
            }
        }

        Mapped { inner: self, map }
    }
}

/// TODO: docs.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Null;

/// TODO: docs.
#[derive(Debug, Copy, Clone)]
pub struct NixValue<'a> {
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

impl NixValue<'_> {
    #[inline]
    pub(crate) fn as_raw(self) -> *mut sys::Value {
        self.ptr.as_ptr()
    }

    /// # Safety
    ///
    /// The caller must ensure that the value has been initialized.
    #[inline]
    pub(crate) unsafe fn new(inner: NonNull<sys::Value>) -> Self {
        Self { ptr: inner, _lifetime: PhantomData }
    }

    /// Calls the given callback with the string held by this value.
    ///
    /// # Safety
    ///
    /// The caller must first ensure that this value's kind is
    /// [`ValueKind::String`].
    unsafe fn with_string(
        &self,
        mut fun: impl FnMut(&CStr),
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe extern "C" fn get_string_callback(
            start: *const c_char,
            n: c_uint,
            fun_ref: *mut c_void,
        ) {
            let num_bytes_including_nul = n + 1;
            let bytes = unsafe {
                slice::from_raw_parts(
                    start as *const u8,
                    num_bytes_including_nul as usize,
                )
            };
            let cstr = unsafe { CStr::from_bytes_with_nul_unchecked(bytes) };
            let fun = unsafe { &mut **(fun_ref as *mut &mut dyn FnMut(&CStr)) };
            fun(cstr);
        }

        let mut fun_ref = &mut fun as &mut dyn FnMut(&CStr);

        ctx.with_raw(|ctx| unsafe {
            sys::get_string(
                ctx,
                self.as_raw(),
                Some(get_string_callback),
                &mut fun_ref as *mut &mut dyn FnMut(&CStr) as *mut c_void,
            );
        })
    }
}

impl Value for Null {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::Null
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        _: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        ctx.with_raw(|ctx| unsafe {
            sys::init_null(ctx, dest.as_ptr());
        })
    }
}

impl Value for NixValue<'_> {
    #[inline]
    fn force_inline(&mut self, ctx: &mut Context) -> Result<()> {
        ctx.with_raw_and_state(|ctx, state| unsafe {
            cpp::force_value(ctx, state.as_ptr(), self.as_raw());
        })
    }

    #[inline]
    fn kind(&self) -> ValueKind {
        // 'nix_get_type' errors when the value pointer is null or when the
        // value is not initizialized, but having a NixValue guarantees neither
        // of those can happen, so we can use a null context.
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
        _: impl Namespace,
        _: &mut Context,
    ) -> Result<()> {
        // 'nix_copy_value' errors when:
        //
        // 1. the destination pointer is null;
        // 2. the destination value is already initialized;
        // 3. the source pointer is null;
        // 4. the source value is not initialized.
        //
        // Having a NixValue guarantees that (3) and (4) cannot happen, having
        // a NonNull destination pointer guarantees that (1) cannot happen, and
        // the API contract for this method guarantees that (2) cannot happen,
        // so we can use a null context.
        unsafe {
            sys::copy_value(ptr::null_mut(), dest.as_ptr(), self.as_raw());
        };
        Ok(())
    }
}

impl BoolValue for NixValue<'_> {
    #[inline]
    unsafe fn into_bool(self, _: &mut Context) -> Result<bool> {
        Ok(unsafe { sys::get_bool(ptr::null_mut(), self.as_raw()) })
    }
}

impl IntValue for NixValue<'_> {
    #[inline]
    unsafe fn into_int(self, _: &mut Context) -> Result<i64> {
        Ok(unsafe { sys::get_int(ptr::null_mut(), self.as_raw()) })
    }
}

impl<'a> StringValue for NixValue<'a> {
    type String = CString;

    #[inline]
    unsafe fn into_string(self, ctx: &mut Context) -> Result<Self::String> {
        let mut cstring = CString::default();
        unsafe { self.with_string(|cstr| cstring = cstr.to_owned(), ctx)? };
        Ok(cstring)
    }
}

impl<'a> PathValue for NixValue<'a> {
    type Path = &'a CStr;

    #[inline]
    unsafe fn into_path_string(self, _: &mut Context) -> Result<Self::Path> {
        let cstr_ptr =
            unsafe { sys::get_path_string(ptr::null_mut(), self.as_raw()) };

        // SAFETY: the [docs] guarantee that the returned pointer is
        // valid for as long as the value is alive.
        //
        // [docs]: https://hydra.nixos.org/build/313564006/download/1/html/group__value__extract.html#ga3420055c22accfd07cc5537210d748a9
        Ok(unsafe { CStr::from_ptr(cstr_ptr) })
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
        _: impl Namespace,
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
                _: impl Namespace,
                ctx: &mut Context,
            ) -> Result<()> {
                ctx.with_raw(|ctx| unsafe {
                    sys::init_int(ctx, dest.as_ptr(), (*self).into());
                })
            }
        }

        impl IntValue for $ty {
            #[inline]
            unsafe fn into_int(self, _: &mut Context) -> Result<i64> {
                Ok(self.into())
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

macro_rules! impl_value_for_big_int {
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
                namespace: impl Namespace,
                ctx: &mut Context,
            ) -> Result<()> {
                unsafe { self.into_int(ctx)?.write(dest, namespace, ctx) }
            }
        }

        impl IntValue for $ty {
            #[inline]
            unsafe fn into_int(self, _: &mut Context) -> Result<i64> {
                self.try_into()
                    .map_err(|_| TryIntoI64Error::<$ty>::new(self).into())
            }
        }
    };
}

impl_value_for_big_int!(usize);
impl_value_for_big_int!(isize);
impl_value_for_big_int!(u64);

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
                _: impl Namespace,
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
        _: impl Namespace,
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
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { self.as_c_str().write(dest, namespace, ctx) }
    }
}

impl Value for str {
    #[inline(always)]
    fn kind(&self) -> ValueKind {
        ValueKind::String
    }

    #[inline(always)]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        let string = CString::new(self)?;
        unsafe { string.write(dest, namespace, ctx) }
    }
}

impl Value for &str {
    #[inline(always)]
    fn kind(&self) -> ValueKind {
        ValueKind::String
    }

    #[inline(always)]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { (*self).write(dest, namespace, ctx) }
    }
}

impl Value for alloc::string::String {
    #[inline(always)]
    fn kind(&self) -> ValueKind {
        ValueKind::String
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { self.as_str().write(dest, namespace, ctx) }
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
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        match self {
            Some(value) => unsafe { value.write(dest, namespace, ctx) },
            None => unsafe { Null.write(dest, namespace, ctx) },
        }
    }
}

impl<P: PrimOp> Value for P {
    #[inline]
    fn kind(&self) -> ValueKind {
        PrimOpImpl::value_kind(self)
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        ctx.write_primop::<Self>(namespace, dest)
    }
}

impl<T: Value + ?Sized + ToOwned> Value for Cow<'_, T> {
    #[inline]
    fn kind(&self) -> ValueKind {
        self.deref().kind()
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { self.deref().write(dest, namespace, ctx) }
    }
}

impl<T: ToValue> Value for Vec<T> {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::List
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { self.as_slice().write(dest, namespace, ctx) }
    }
}

impl<T: ToValue> Value for &[T] {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::List
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { self.into_list().into_value().write(dest, namespace, ctx) }
    }
}

impl<const N: usize, T: ToValue> Value for [T; N] {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::List
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { self.into_list().into_value().write(dest, namespace, ctx) }
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
        _: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        let bytes = self.as_os_str().as_encoded_bytes();
        let cstring = CString::new(bytes)?;
        unsafe {
            cpp::init_path_string(
                ctx.state_mut().as_ptr(),
                dest.as_ptr(),
                cstring.as_ptr(),
            );
        }
        Ok(())
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
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { (*self).write(dest, namespace, ctx) }
    }
}

#[cfg(feature = "std")]
impl PathValue for &std::path::Path {
    type Path = CString;

    #[inline]
    unsafe fn into_path_string(self, _: &mut Context) -> Result<Self::Path> {
        CString::new(self.as_os_str().as_encoded_bytes()).map_err(Into::into)
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
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { self.as_path().write(dest, namespace, ctx) }
    }
}

#[cfg(feature = "std")]
impl PathValue for std::path::PathBuf {
    type Path = CString;

    #[inline]
    unsafe fn into_path_string(self, ctx: &mut Context) -> Result<Self::Path> {
        unsafe { self.as_path().into_path_string(ctx) }
    }
}

#[cfg(feature = "std")]
impl Value for std::ffi::OsStr {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::String
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        let bytes = self.as_encoded_bytes();
        let cstring = CString::new(bytes)?;
        unsafe { cstring.write(dest, namespace, ctx) }
    }
}

#[cfg(feature = "std")]
impl Value for &std::ffi::OsStr {
    #[inline(always)]
    fn kind(&self) -> ValueKind {
        (*self).kind()
    }

    #[inline(always)]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { (*self).write(dest, namespace, ctx) }
    }
}

#[cfg(feature = "compact_str")]
impl Value for compact_str::CompactString {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::String
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { self.as_str().write(dest, namespace, ctx) }
    }
}

#[cfg(feature = "smallvec")]
impl<T: ToValue, const N: usize> Value for smallvec::SmallVec<[T; N]> {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::List
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { self.as_slice().write(dest, namespace, ctx) }
    }
}

#[cfg(feature = "compact_str")]
impl TryFromValue<NixValue<'_>> for compact_str::CompactString {
    #[inline]
    fn try_from_value(
        mut value: NixValue<'_>,
        ctx: &mut Context,
    ) -> Result<Self> {
        value.force_inline(ctx)?;

        match value.kind() {
            ValueKind::String => {
                let mut res = Ok(Self::const_new(""));
                // SAFETY: the value's kind is a string.
                unsafe {
                    value.with_string(
                        |cstr| res = cstr.to_str().map(Into::into),
                        ctx,
                    )?
                };
                res.map_err(Into::into)
            },
            other => Err(TypeMismatchError {
                expected: ValueKind::String,
                found: other,
            }
            .into()),
        }
    }
}

#[cfg(feature = "either")]
impl<L: Value, R: Value> Value for either::Either<L, R> {
    #[inline]
    fn kind(&self) -> ValueKind {
        match self {
            Self::Left(left) => left.kind(),
            Self::Right(right) => right.kind(),
        }
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        match self {
            Self::Left(left) => unsafe { left.write(dest, namespace, ctx) },
            Self::Right(right) => unsafe { right.write(dest, namespace, ctx) },
        }
    }
}

impl<T: Value> IntoValue for T {
    #[inline(always)]
    fn into_value(self, _: &mut Context) -> impl Value + use<T> {
        self
    }
}

impl<T: Value> ToValue for T {
    #[inline(always)]
    fn to_value<'a>(&'a self, _: &mut Context) -> impl Value + use<'a, T> {
        Value::borrow(self)
    }
}

impl<T: IntoValue> TryIntoValue for T {
    #[inline(always)]
    fn try_into_value<'eval>(
        self,
        ctx: &mut Context<'eval>,
    ) -> Result<impl Value + use<'eval, T>> {
        Ok(self.into_value(ctx))
    }
}

impl<T: TryIntoValue, E: Into<Error>> TryIntoValue
    for core::result::Result<T, E>
{
    #[inline]
    fn try_into_value<'eval>(
        self,
        ctx: &mut Context<'eval>,
    ) -> Result<impl Value + use<'eval, T, E>> {
        self.map_err(Into::into).and_then(|value| value.try_into_value(ctx))
    }
}

macro_rules! impl_try_from_value_for_self {
    ($ty:ty) => {
        impl TryFromValue<Self> for $ty {
            #[inline]
            fn try_from_value(value: Self, _: &mut Context) -> Result<Self> {
                Ok(value)
            }
        }
    };
}

impl_try_from_value_for_self!(NixValue<'_>);

impl<V: BoolValue> TryFromValue<V> for bool {
    #[inline]
    fn try_from_value(mut value: V, ctx: &mut Context) -> Result<Self> {
        value.force_inline(ctx)?;

        match value.kind() {
            // SAFETY: the value's kind is a boolean.
            ValueKind::Bool => unsafe { value.into_bool(ctx) },
            other => Err(TypeMismatchError {
                expected: ValueKind::Bool,
                found: other,
            }
            .into()),
        }
    }
}

impl<V: IntValue> TryFromValue<V> for i64 {
    #[inline]
    fn try_from_value(mut value: V, ctx: &mut Context) -> Result<Self> {
        value.force_inline(ctx)?;

        match value.kind() {
            // SAFETY: the value's kind is an integer.
            ValueKind::Int => unsafe { value.into_int(ctx) },
            other => Err(TypeMismatchError {
                expected: ValueKind::Int,
                found: other,
            }
            .into()),
        }
    }
}

macro_rules! impl_try_from_value_for_int {
    ($ty:ty) => {
        impl<V: IntValue> TryFromValue<V> for $ty {
            #[inline]
            fn try_from_value(value: V, ctx: &mut Context) -> Result<Self> {
                let int = i64::try_from_value(value, ctx)?;

                int.try_into()
                    .map_err(|_| TryFromI64Error::<$ty>::new(int).into())
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

macro_rules! impl_try_from_string_value {
    ($ty:ty) => {
        impl<V: StringValue> TryFromValue<V> for $ty
        where
            V::String: TryInto<Self, Error: Into<Error>>,
        {
            #[inline]
            fn try_from_value(mut value: V, ctx: &mut Context) -> Result<Self> {
                value.force_inline(ctx)?;

                match value.kind() {
                    ValueKind::String => {
                        // SAFETY: the value's kind is a string.
                        let string = unsafe { value.into_string(ctx)? };
                        string.try_into().map_err(Into::into)
                    },
                    other => Err(TypeMismatchError {
                        expected: ValueKind::String,
                        found: other,
                    }
                    .into()),
                }
            }
        }
    };
}

impl_try_from_string_value!(&CStr);
impl_try_from_string_value!(&str);
impl_try_from_string_value!(CString);
impl_try_from_string_value!(alloc::string::String);

impl<'a, T> TryFromValue<NixList<'a>> for Vec<T>
where
    T: TryFromValue<NixValue<'a>>,
{
    #[inline]
    fn try_from_value(list: NixList<'a>, ctx: &mut Context) -> Result<Self> {
        (0..list.len()).map(|idx| list.get(idx, ctx)).collect()
    }
}

impl<'a, T> TryFromValue<NixValue<'a>> for Vec<T>
where
    T: TryFromValue<NixValue<'a>>,
{
    #[inline]
    fn try_from_value(value: NixValue<'a>, ctx: &mut Context) -> Result<Self> {
        NixList::try_from_value(value, ctx)
            .and_then(|list| Self::try_from_value(list, ctx))
    }
}

impl<'a, T> TryFromValue<NixValue<'a>> for Option<T>
where
    T: TryFromValue<NixValue<'a>>,
{
    #[inline]
    fn try_from_value(value: NixValue<'a>, ctx: &mut Context) -> Result<Self> {
        T::try_from_value(value, ctx).map(Some)
    }
}

#[cfg(all(unix, feature = "std"))]
impl<'a, V: PathValue<Path = &'a CStr>> TryFromValue<V>
    for &'a std::path::Path
{
    #[inline]
    fn try_from_value(mut value: V, ctx: &mut Context) -> Result<Self> {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        use std::path::Path;

        value.force_inline(ctx)?;

        match value.kind() {
            ValueKind::Path => {
                // SAFETY: the value's kind is a path.
                let cstr = unsafe { value.into_path_string(ctx)? };
                let os_str = OsStr::from_bytes(cstr.to_bytes());
                Ok(Path::new(os_str))
            },
            other => Err(TypeMismatchError {
                expected: ValueKind::Path,
                found: other,
            }
            .into()),
        }
    }
}

#[cfg(all(unix, feature = "std"))]
impl<'a, V: PathValue> TryFromValue<V> for Cow<'a, std::path::Path>
where
    V::Path: Into<Cow<'a, CStr>>,
{
    #[inline]
    fn try_from_value(mut value: V, ctx: &mut Context) -> Result<Self> {
        use alloc::borrow::Cow;
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        use std::path::Path;

        value.force_inline(ctx)?;

        match value.kind() {
            ValueKind::Path => {
                // SAFETY: the value's kind is a path.
                match unsafe { value.into_path_string(ctx)? }.into() {
                    Cow::Borrowed(cstr) => {
                        let os_str = OsStr::from_bytes(cstr.to_bytes());
                        Ok(Cow::Borrowed(Path::new(os_str)))
                    },
                    Cow::Owned(cstring) => {
                        let os_str = OsStr::from_bytes(cstring.to_bytes());
                        Ok(Cow::Owned(Path::new(os_str).to_owned()))
                    },
                }
            },
            other => Err(TypeMismatchError {
                expected: ValueKind::Path,
                found: other,
            }
            .into()),
        }
    }
}

#[cfg(feature = "std")]
impl<'a, V: PathValue> TryFromValue<V> for std::path::PathBuf
where
    V::Path: Into<Cow<'a, CStr>>,
{
    #[inline]
    fn try_from_value(value: V, ctx: &mut Context) -> Result<Self> {
        <Cow<'_, std::path::Path>>::try_from_value(value, ctx)
            .map(Cow::into_owned)
    }
}

#[rustfmt::skip]
mod values_impls {
    use super::*;

    impl<V: Value> Values for V {
        const LEN: c_uint = 1;

        #[track_caller]
        #[inline]
        fn with_value<T>(
            &self,
            value_idx: c_uint,
            fun: impl FnOnceValue<T>,
        ) -> T {
            match value_idx {
                0 => fun.call(self.borrow(), ()),
                other => panic_tuple_index_oob(other, <Self as Values>::LEN),
            }
        }
    }

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
                        $($idx => _fun.call(self.$idx.borrow(), ()),)*
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

    #[track_caller]
    #[inline(never)]
    fn panic_tuple_index_oob(idx: c_uint, len: c_uint) -> ! {
        panic!("{len}-tuple received out of bounds index: {idx}")
    }
}
