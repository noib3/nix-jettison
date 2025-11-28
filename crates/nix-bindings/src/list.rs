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
pub trait List {
    /// Returns a [`List`] implementation that borrows from `self`.
    #[inline]
    fn borrow(&self) -> impl List {
        struct BorrowedList<'a, T: ?Sized> {
            inner: &'a T,
        }

        impl<T: List + ?Sized> List for BorrowedList<'_, T> {
            #[inline]
            fn borrow(&self) -> impl List {
                BorrowedList { inner: self.inner }
            }

            #[inline]
            fn len(&self) -> c_uint {
                self.inner.len()
            }

            #[inline]
            fn with_value<V>(
                &self,
                idx: c_uint,
                fun: impl FnOnceValue<V>,
                ctx: &mut Context,
            ) -> Result<V> {
                self.inner.with_value(idx, fun, ctx)
            }
        }

        BorrowedList { inner: self }
    }

    /// Returns the number of elements in this list.
    fn len(&self) -> c_uint;

    /// Returns whether this list is empty.
    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// TODO: docs.
    fn with_value<T>(
        &self,
        idx: c_uint,
        fun: impl FnOnceValue<T>,
        ctx: &mut Context,
    ) -> Result<T>;
}

/// The list type produced by the [`list!`] macro.
pub struct LiteralList<Values> {
    values: Values,
}

impl<Values> LiteralList<Values> {
    /// Creates a new `LiteralList`.
    #[inline]
    pub fn new(values: Values) -> Self {
        Self { values }
    }
}

impl<V> Deref for LiteralList<V> {
    type Target = V;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl<V: Values> List for LiteralList<V> {
    #[inline]
    fn len(&self) -> c_uint {
        V::LEN
    }

    #[inline]
    fn with_value<T>(
        &self,
        idx: c_uint,
        fun: impl FnOnceValue<T>,
        _: &mut Context,
    ) -> Result<T> {
        Ok(self.values.with_value(idx, fun))
    }
}

impl<V: Values> Value for LiteralList<V> {
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

        let len = self.len();

        let mut builder = ctx.make_list_builder(len as usize)?;

        for idx in 0..len {
            // FIXME: avoid this allocation.
            let idx_cstr = CString::new(idx.to_string()).expect("no NUL byte");
            let new_namespace = namespace.push(&idx_cstr);
            builder.insert(|dest, ctx| {
                self.values.with_value(
                    idx,
                    WriteValue { dest, namespace: new_namespace, ctx },
                )
            })?;
            namespace = new_namespace.pop();
        }

        builder.build(dest)
    }
}

impl<T, V> List for T
where
    T: Deref<Target = [V]>,
    V: Value,
{
    #[inline]
    fn len(&self) -> c_uint {
        self.deref().len() as c_uint
    }

    #[inline]
    fn with_value<U>(
        &self,
        idx: c_uint,
        fun: impl FnOnceValue<U>,
        _: &mut Context,
    ) -> Result<U> {
        Ok(fun.call(self.deref()[idx as usize].borrow()))
    }
}
