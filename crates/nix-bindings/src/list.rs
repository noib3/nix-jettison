//! TODO: docs.

use alloc::ffi::CString;
use alloc::string::ToString;
use core::cell::Cell;
use core::ffi::c_uint;
use core::ops::Deref;
use core::ptr::{self, NonNull};

pub use nix_bindings_macros::list;
use nix_bindings_sys as sys;

use crate::error::TypeMismatchError;
use crate::namespace::{Namespace, PoppableNamespace};
use crate::prelude::{Context, Result, Value, ValueKind};
use crate::value::{FnOnceValue, NixValue, ToValue, TryFromValue, Values};

/// TODO: docs.
pub trait List {
    /// Returns the number of elements in this list.
    fn len(&self) -> c_uint;

    /// TODO: docs.
    fn with_value<'ctx, 'eval, T>(
        &self,
        idx: c_uint,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> T;

    /// Returns a [`List`] implementation that borrows from `self`.
    #[inline]
    fn borrow(&self) -> impl List {
        struct BorrowedList<'a, T: ?Sized> {
            inner: &'a T,
        }

        impl<T: List + ?Sized> List for BorrowedList<'_, T> {
            #[inline]
            fn borrow(&self) -> impl List {
                Self { inner: self.inner }
            }

            #[inline]
            fn len(&self) -> c_uint {
                self.inner.len()
            }

            #[inline]
            fn with_value<'ctx, 'eval, V>(
                &self,
                idx: c_uint,
                fun: impl FnOnceValue<V, &'ctx mut Context<'eval>>,
                ctx: &'ctx mut Context<'eval>,
            ) -> V {
                self.inner.with_value(idx, fun, ctx)
            }
        }

        BorrowedList { inner: self }
    }

    /// TODO: docs.
    #[inline(always)]
    fn into_list(self) -> impl List
    where
        Self: Sized,
    {
        self
    }

    /// TODO: docs.
    #[inline]
    fn into_value(self) -> impl Value
    where
        Self: Sized,
    {
        ListValue(self)
    }

    /// Returns whether this list is empty.
    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// An extension trait for iterators of [`Value`]s.
pub trait IteratorExt: IntoIterator<Item: Value> {
    /// TODO: docs.
    fn into_value(self) -> impl Value
    where
        Self: Sized,
        Self::IntoIter: ExactSizeIterator + Clone;
}

/// TODO: docs.
#[derive(Copy, Clone)]
pub struct NixList<'value> {
    inner: NixValue<'value>,
}

/// The list type produced by the [`list!`] macro.
pub struct LiteralList<Values> {
    values: Values,
}

/// A hybrid trait between a [`List`] and an [`Iterator`] over values, with a
/// more relaxed interface than either.
trait ValueIterator {
    fn initial_len(&self) -> c_uint;

    fn with_next_value<'ctx, 'eval, T>(
        &self,
        idx: c_uint,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> T;
}

/// A newtype wrapper that implements [`Value`] for every [`ValueIterator`].
struct ListValue<T>(T);

impl<'a> NixList<'a> {
    /// TODO: docs.
    #[inline]
    pub fn get<T: TryFromValue<NixValue<'a>>>(
        self,
        idx: c_uint,
        ctx: &mut Context,
    ) -> Result<T> {
        self.with_value_inner(
            idx,
            |value, ctx| T::try_from_value(value, ctx),
            ctx,
        )
    }

    #[inline]
    fn with_value_inner<'ctx, 'eval, T>(
        self,
        idx: c_uint,
        fun: impl FnOnce(NixValue<'a>, &'ctx mut Context<'eval>) -> T,
        ctx: &'ctx mut Context<'eval>,
    ) -> T {
        let value_raw = unsafe {
            sys::get_list_byidx_lazy(
                ptr::null_mut(),
                self.inner.as_raw(),
                ctx.state_mut().as_ptr(),
                idx,
            )
        };

        let value_ptr =
            NonNull::new(value_raw).expect("Nix returned null value");

        // SAFETY: the value returned by Nix is initialized.
        fun(unsafe { NixValue::new(value_ptr) }, ctx)
    }
}

impl<Values> LiteralList<Values> {
    /// Creates a new `LiteralList`.
    #[inline]
    pub fn new(values: Values) -> Self {
        Self { values }
    }
}

impl List for NixList<'_> {
    #[inline]
    fn into_value(self) -> impl Value
    where
        Self: Sized,
    {
        self
    }

    #[inline]
    fn len(&self) -> c_uint {
        // 'nix_get_list_size' errors when the value pointer is null or when
        // the value is not initizialized, but having a NixValue guarantees
        // neither of those can happen, so we can use a null context.
        unsafe { sys::get_list_size(ptr::null_mut(), self.inner.as_raw()) }
    }

    #[inline]
    fn with_value<'ctx, 'eval, T>(
        &self,
        idx: c_uint,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> T {
        self.with_value_inner(idx, |value, ctx| fun.call(value, ctx), ctx)
    }
}

impl Value for NixList<'_> {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::Attrset
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

impl<'a> TryFromValue<NixValue<'a>> for NixList<'a> {
    #[inline]
    fn try_from_value(
        mut value: NixValue<'a>,
        ctx: &mut Context,
    ) -> Result<Self> {
        value.force_inline(ctx)?;

        match value.kind() {
            ValueKind::List => Ok(Self { inner: value }),
            other => Err(ctx.make_error(TypeMismatchError {
                expected: ValueKind::List,
                found: other,
            })),
        }
    }
}

impl<'a> From<NixList<'a>> for NixValue<'a> {
    #[inline]
    fn from(list: NixList<'a>) -> Self {
        list.inner
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
    fn with_value<'ctx, 'eval, T>(
        &self,
        idx: c_uint,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> T {
        self.values.with_value(idx, fun.map_ctx(move |()| ctx))
    }
}

impl<V: Values> Value for LiteralList<V> {
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
        unsafe { List::borrow(self).into_value().write(dest, namespace, ctx) }
    }
}

impl<L: ValueIterator> Value for ListValue<L> {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::List
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        mut namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        struct WriteValue<N> {
            dest: NonNull<sys::Value>,
            namespace: N,
        }

        impl<N: Namespace> FnOnceValue<Result<()>, &mut Context<'_>> for WriteValue<N> {
            #[inline]
            fn call(self, value: impl Value, ctx: &mut Context) -> Result<()> {
                unsafe { value.write(self.dest, self.namespace, ctx) }
            }
        }

        let Self(iter) = self;

        let len = iter.initial_len();

        let mut builder = ctx.make_list_builder(len as usize)?;

        for idx in 0..len {
            // FIXME: avoid this allocation.
            let idx_cstr = CString::new(idx.to_string()).expect("no NUL byte");
            let new_namespace = namespace.push(&idx_cstr);
            builder.insert(|dest, ctx| {
                iter.with_next_value(
                    idx,
                    WriteValue { dest, namespace: new_namespace },
                    ctx,
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
    V: ToValue,
{
    #[inline]
    fn len(&self) -> c_uint {
        self.deref().len() as c_uint
    }

    #[inline]
    fn with_value<'ctx, 'eval, U>(
        &self,
        idx: c_uint,
        fun: impl FnOnceValue<U, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> U {
        fun.call(self.deref()[idx as usize].to_value(), ctx)
    }
}

impl<I: IntoIterator<Item: Value>> IteratorExt for I {
    #[inline]
    fn into_value(self) -> impl Value
    where
        Self::IntoIter: ExactSizeIterator + Clone,
    {
        struct RewindIter<Iter> {
            current: Cell<Option<Iter>>,
            orig: Iter,
        }

        impl<Iter: Clone> RewindIter<Iter> {
            #[inline]
            fn with_iter<T>(
                &self,
                fun: impl FnOnce(&mut Iter) -> (T, bool),
            ) -> T {
                // SAFETY: the inner Cell always contains Some(I).
                let mut iter =
                    unsafe { self.current.take().unwrap_unchecked() };

                let (out, should_rewind) = fun(&mut iter);

                let new_iter =
                    if should_rewind { self.orig.clone() } else { iter };

                self.current.set(Some(new_iter));

                out
            }
        }

        impl<I: ExactSizeIterator + Clone> ValueIterator for RewindIter<I>
        where
            I::Item: Value,
        {
            #[inline]
            fn initial_len(&self) -> c_uint {
                self.orig.len() as c_uint
            }

            #[inline]
            fn with_next_value<'ctx, 'eval, T>(
                &self,
                _: c_uint,
                fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
                ctx: &'ctx mut Context<'eval>,
            ) -> T {
                self.with_iter(|iter| {
                    let Some(value) = iter.next() else {
                        panic!(
                            "ValueIterator::with_next_value() called more \
                             times than advertised by initial_len()"
                        );
                    };
                    (fun.call(value, ctx), iter.len() == 0)
                })
            }
        }

        let iter = self.into_iter();

        ListValue(RewindIter {
            current: Cell::new(Some(iter.clone())),
            orig: iter,
        })
    }
}

impl<L: List> ValueIterator for L {
    #[inline]
    fn initial_len(&self) -> c_uint {
        L::len(self)
    }

    #[inline]
    fn with_next_value<'ctx, 'eval, T>(
        &self,
        idx: c_uint,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> T {
        self.with_value(idx, fun, ctx)
    }
}
