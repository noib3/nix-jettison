//! TODO: docs.

use alloc::ffi::CString;
use alloc::string::ToString;
use core::ffi::c_uint;
use core::ops::Deref;
use core::ptr::NonNull;

pub use nix_bindings_macros::list;
use nix_bindings_sys as sys;

use crate::namespace::{Namespace, PoppableNamespace};
use crate::prelude::{Context, Result, Value, ValueKind};
use crate::value::{FnOnceValue, Values};

/// TODO: docs.
pub trait List: Sized {
    /// Returns a [`List`] implementation that borrows from `self`.
    #[inline]
    fn borrow(&self) -> impl List {
        struct BorrowedList<'a, T> {
            inner: &'a T,
        }

        impl<T: List> List for BorrowedList<'_, T> {
            #[inline]
            fn get_value_kind(&self, idx: c_uint) -> ValueKind {
                self.inner.get_value_kind(idx)
            }

            #[inline]
            fn len(&self) -> c_uint {
                self.inner.len()
            }

            #[inline]
            unsafe fn write_value(
                &self,
                idx: c_uint,
                dest: NonNull<sys::Value>,
                namespace: impl Namespace,
                ctx: &mut Context,
            ) -> Result<()> {
                unsafe { self.inner.write_value(idx, dest, namespace, ctx) }
            }
        }

        BorrowedList { inner: self }
    }

    /// Returns the [`ValueKind`] of the value at the given index.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds (i.e. greater than or equal to
    /// `self.len()`).
    fn get_value_kind(&self, idx: c_uint) -> ValueKind;

    /// Returns the number of elements in this list.
    fn len(&self) -> c_uint;

    /// TODO: docs.
    #[inline]
    fn into_value(self) -> impl Value {
        ListValue(self)
    }

    /// Returns whether this list is empty.
    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Writes the value of the attribute at the given index into the given
    /// destination pointer.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `dest` points to a valid, uninitialized
    /// `sys::Value` instance.
    #[allow(clippy::too_many_arguments)]
    unsafe fn write_value(
        &self,
        idx: c_uint,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()>;
}

/// The list type produced by the [`list!`] macro.
pub struct LiteralList<Values> {
    values: Values,
}

/// A newtype wrapper that implements `Value` for every `List`.
struct ListValue<T>(T);

impl<Values> LiteralList<Values> {
    /// Creates a new `LiteralList`.
    #[inline]
    pub fn new(values: Values) -> Self {
        Self { values }
    }
}

impl<V: Values> List for LiteralList<V> {
    #[inline]
    fn get_value_kind(&self, idx: c_uint) -> ValueKind {
        struct GetValueKind;
        impl FnOnceValue<ValueKind> for GetValueKind {
            fn call(self, value: impl Value) -> ValueKind {
                value.kind()
            }
        }
        self.with_value(idx, GetValueKind)
    }

    #[inline]
    fn len(&self) -> c_uint {
        V::LEN
    }

    #[inline]
    unsafe fn write_value(
        &self,
        idx: c_uint,
        dest: NonNull<nix_bindings_sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        struct WriteValue<'ctx, N> {
            dest: NonNull<sys::Value>,
            namespace: N,
            ctx: &'ctx mut Context,
        }
        impl<N: Namespace> FnOnceValue<Result<()>> for WriteValue<'_, N> {
            fn call(self, value: impl Value) -> Result<()> {
                unsafe {
                    value.write_with_namespace(
                        self.dest,
                        self.namespace,
                        self.ctx,
                    )
                }
            }
        }
        self.with_value(idx, WriteValue { dest, namespace, ctx })
    }
}

impl<V> Deref for LiteralList<V> {
    type Target = V;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl<Values> Value for LiteralList<Values>
where
    Self: List,
{
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::List
    }

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
        unsafe {
            List::borrow(self)
                .into_value()
                .write_with_namespace(dest, namespace, ctx)
        }
    }
}

impl<T, V> List for T
where
    T: Deref<Target = [V]>,
    V: Value,
{
    #[inline]
    fn get_value_kind(&self, idx: c_uint) -> ValueKind {
        self.deref()[idx as usize].kind()
    }

    #[inline]
    fn len(&self) -> c_uint {
        self.deref().len() as c_uint
    }

    #[inline]
    unsafe fn write_value(
        &self,
        idx: c_uint,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe {
            self.deref()[idx as usize]
                .write_with_namespace(dest, namespace, ctx)
        }
    }
}

impl<T: List> Value for ListValue<T> {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::List
    }

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
        mut namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        let Self(list) = self;

        unsafe {
            let len = list.len();
            let mut builder = ctx.make_list_builder(len as usize)?;
            for idx in 0..len {
                // FIXME: avoid this allocation.
                let idx_cstr =
                    CString::new(idx.to_string()).expect("no NUL byte");
                let new_namespace = namespace.push(&idx_cstr);
                builder.insert(|dest, ctx| {
                    list.write_value(idx, dest, new_namespace, ctx)
                })?;
                namespace = new_namespace.pop();
            }
            builder.build(dest)
        }
    }
}
