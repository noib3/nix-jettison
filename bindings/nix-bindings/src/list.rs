//! TODO: docs.

use alloc::ffi::CString;
use alloc::string::ToString;
use core::cell::{Cell, OnceCell};
use core::ffi::c_uint;
use core::ops::Deref;
use core::ptr::{self, NonNull};

pub use nix_bindings_macros::list;
use nix_bindings_sys as sys;

use crate::error::TypeMismatchError;
use crate::namespace::{Namespace, PoppableNamespace};
use crate::prelude::{Context, Result, Value, ValueKind};
use crate::value::{
    FnOnceValue,
    IntoValue,
    NixValue,
    ToValue,
    TryFromValue,
    Values,
};

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
        struct Wrapper<'a, L: ?Sized> {
            list: &'a L,
        }

        impl<L: List + ?Sized> List for Wrapper<'_, L> {
            #[inline]
            fn borrow(&self) -> impl List {
                Self { list: self.list }
            }

            #[inline]
            fn len(&self) -> c_uint {
                self.list.len()
            }

            #[inline]
            fn with_value<'ctx, 'eval, V>(
                &self,
                idx: c_uint,
                fun: impl FnOnceValue<V, &'ctx mut Context<'eval>>,
                ctx: &'ctx mut Context<'eval>,
            ) -> V {
                self.list.with_value(idx, fun, ctx)
            }
        }

        Wrapper { list: self }
    }

    /// TODO: docs.
    #[inline]
    fn concat<T: List>(self, other: T) -> Concat<Self, T>
    where
        Self: Sized,
    {
        Concat { left: self, right: other }
    }

    /// TODO: docs.
    #[inline(always)]
    fn for_each<'eval>(
        &self,
        fun: impl for<'a> FnOnceValue<Result<()>, &'a mut Context<'eval>> + Clone,
        ctx: &mut Context<'eval>,
    ) -> Result<()> {
        for idx in 0..self.len() {
            self.with_value(idx, fun.clone(), ctx)?;
        }
        Ok(())
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
        struct Wrapper<L>(L);

        impl<L: List> Value for Wrapper<L> {
            #[inline(always)]
            fn kind(&self) -> ValueKind {
                ValueKind::List
            }

            #[inline(always)]
            unsafe fn write(
                &self,
                dest: NonNull<nix_bindings_sys::Value>,
                namespace: impl Namespace,
                ctx: &mut Context,
            ) -> Result<()> {
                unsafe {
                    WriteableList::write_once(
                        self.0.as_writeable(),
                        dest,
                        namespace,
                        ctx,
                    )
                }
            }
        }

        Wrapper(self)
    }

    /// Returns whether this list is empty.
    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a [`WriteableList`] implementation over this list.
    #[inline(always)]
    #[doc(hidden)]
    fn as_writeable(&self) -> impl WriteableList {
        struct Wrapper<'a, L: ?Sized> {
            list: &'a L,
            index: c_uint,
        }

        impl<L: List + ?Sized> WriteableList for Wrapper<'_, L> {
            #[inline]
            fn initial_len(&self) -> c_uint {
                self.list.len()
            }

            #[inline]
            fn with_next<'ctx, 'eval, T>(
                &mut self,
                fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
                ctx: &'ctx mut Context<'eval>,
            ) -> T {
                let out = self.list.with_value(self.index, fun, ctx);
                self.index += 1;
                out
            }
        }

        Wrapper { list: self, index: 0 }
    }
}

/// An extension trait for iterators of [`IntoValue`]s.
pub trait IteratorExt: IntoIterator<IntoIter: ExactSizeIterator> {
    /// TODO: docs.
    fn into_value(self) -> impl Value
    where
        Self: Sized,
        Self::Item: IntoValue;

    /// Chains two [`ExactSizeIterator`]s together, returning a new
    /// [`ExactSizeIterator`] that will iterate over both.
    ///
    /// See the discussion in https://github.com/rust-lang/rust/issues/34433
    /// for why [`Chain`](std::iter::Chain) doesn't already do this.
    ///
    /// # Panics
    ///
    /// The [`ExactSizeIterator::len`] implementation of the returned iterator
    /// will panic if the sum of the two iterators' lengths overflows a
    /// `usize`.
    #[inline]
    fn chain_exact<T>(self, other: T) -> ChainExact<Self::IntoIter, T::IntoIter>
    where
        Self: Sized,
        T: IntoIterator<IntoIter: ExactSizeIterator<Item = Self::Item>>,
    {
        ChainExact {
            left: Some(self.into_iter()),
            right: Some(other.into_iter()),
        }
    }
}

/// TODO: docs.
#[derive(Debug, Copy, Clone)]
pub struct NixList<'value> {
    inner: NixValue<'value>,
}

/// The list type produced by the [`list!`] macro.
pub struct LiteralList<Values> {
    values: Values,
}

/// TODO: docs.
#[derive(Copy, Clone)]
pub struct Concat<L, R> {
    left: L,
    right: R,
}

/// The iterator type returned by calling [`IteratorExt::chain_exact`].
#[derive(Clone)]
pub struct ChainExact<L, R> {
    left: Option<L>,
    right: Option<R>,
}

/// TODO: docs.
trait WriteableList {
    fn initial_len(&self) -> c_uint;

    fn with_next<'ctx, 'eval, T>(
        &mut self,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> T;

    #[inline]
    unsafe fn write_once(
        mut self,
        dest: NonNull<sys::Value>,
        mut namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()>
    where
        Self: Sized,
    {
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

        let len = self.initial_len();

        let mut builder = ctx.make_list_builder(len as usize)?;

        for idx in 0..len {
            // FIXME: avoid this allocation.
            let idx_cstr = CString::new(idx.to_string()).expect("no NUL byte");
            let new_namespace = namespace.push(&idx_cstr);
            builder.insert(|dest, ctx| {
                self.with_next(
                    WriteValue { dest, namespace: new_namespace },
                    ctx,
                )
            })?;
            namespace = new_namespace.pop();
        }

        builder.build(dest)
    }
}

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
            other => Err(TypeMismatchError {
                expected: ValueKind::List,
                found: other,
            }
            .into()),
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

impl<L, R> List for Concat<L, R>
where
    L: List,
    R: List,
{
    #[inline]
    fn len(&self) -> c_uint {
        self.left.len() + self.right.len()
    }

    #[inline]
    fn with_value<'ctx, 'eval, T>(
        &self,
        idx: c_uint,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> T {
        let left_len = self.left.len();
        if idx < left_len {
            self.left.with_value(idx, fun, ctx)
        } else {
            self.right.with_value(idx - left_len, fun, ctx)
        }
    }
}

impl<L, R> Value for Concat<L, R>
where
    Self: List,
{
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
        fun.call(self.deref()[idx as usize].to_value(ctx), ctx)
    }
}

impl<I: IntoIterator<IntoIter: ExactSizeIterator>> IteratorExt for I {
    #[inline]
    fn into_value(self) -> impl Value
    where
        Self::Item: IntoValue,
    {
        struct Wrapper<Iter> {
            iter: Cell<Option<Iter>>,
            list_res: OnceCell<Result<NixList<'static>>>,
        }

        impl<Iter: WriteableList> Value for Wrapper<Iter> {
            #[inline]
            fn kind(&self) -> ValueKind {
                ValueKind::List
            }

            #[inline]
            unsafe fn write(
                &self,
                dest: NonNull<nix_bindings_sys::Value>,
                namespace: impl Namespace,
                ctx: &mut Context,
            ) -> Result<()> {
                let Some(iter) = self.iter.take() else {
                    let list = self
                        .list_res
                        .get()
                        .expect(
                            "if the iterator has been taken it means \
                             Value::write has already been called, and its \
                             result must've been saved",
                        )
                        .clone()?;
                    return unsafe { list.write(dest, namespace, ctx) };
                };

                let dest = match ctx.alloc_value() {
                    Ok(uninit_value) => uninit_value,
                    Err(err) => {
                        self.iter.set(Some(iter));
                        return Err(err);
                    },
                };

                let list_res = match unsafe {
                    WriteableList::write_once(iter, dest, namespace, ctx)
                } {
                    Ok(()) => {
                        Ok(NixList { inner: unsafe { NixValue::new(dest) } })
                    },
                    Err(err) => {
                        unsafe {
                            sys::value_decref(ptr::null_mut(), dest.as_ptr());
                        }
                        Err(err)
                    },
                };

                self.list_res.set(list_res).expect("not been set before");

                Ok(())
            }
        }

        Wrapper {
            iter: Cell::new(Some(self.into_iter())),
            list_res: OnceCell::new(),
        }
    }
}

impl<I: ExactSizeIterator<Item: IntoValue>> WriteableList for I {
    #[track_caller]
    #[inline]
    fn initial_len(&self) -> c_uint {
        match self.len().try_into() {
            Ok(len) => len,
            Err(_overflow_err) => {
                panic!("iterator has too many elements, max is {}", c_uint::MAX)
            },
        }
    }

    #[inline]
    fn with_next<'ctx, 'eval, T>(
        &mut self,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> T {
        let Some(item) = self.next() else {
            unreachable!(
                "WriteableList::with_next() has been called more times than \
                 advertised by initial_len"
            );
        };
        fun.call(item.into_value(ctx), ctx)
    }
}

impl<L, R> Iterator for ChainExact<L, R>
where
    L: ExactSizeIterator,
    R: ExactSizeIterator<Item = L::Item>,
{
    type Item = L::Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let (item, is_left) = match (&mut self.left, &mut self.right) {
            (Some(left), _) => (left.next(), true),
            (None, Some(right)) => (right.next(), false),
            (None, None) => return None,
        };

        match item {
            Some(item) => Some(item),
            None => {
                if is_left {
                    self.left = None;
                    self.next()
                } else {
                    self.right = None;
                    None
                }
            },
        }
    }

    #[track_caller]
    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let exact = self.len();
        (exact, Some(exact))
    }
}

impl<L, R> ExactSizeIterator for ChainExact<L, R>
where
    L: ExactSizeIterator,
    R: ExactSizeIterator<Item = L::Item>,
{
    #[track_caller]
    #[inline]
    fn len(&self) -> usize {
        self.left.as_ref().map_or(0, |iter| iter.len())
            + self.right.as_ref().map_or(0, |iter| iter.len())
    }
}

/// TODO: docs.
///
/// Needed because we can't implement `List` directly on `Either` because
/// `List` already has a blanket implementation for types that deref to `&[T]`,
/// (where `T` is `ToValue`).
#[cfg(feature = "either")]
pub trait EitherExt {
    /// TODO: docs.
    fn as_list(&self) -> impl List;

    /// TODO: docs.
    fn into_list(self) -> impl List
    where
        Self: Sized;
}

#[cfg(feature = "either")]
impl<L, R> EitherExt for either::Either<L, R>
where
    L: List,
    R: List,
{
    #[inline]
    fn as_list(&self) -> impl List {
        match self {
            Self::Left(left) => either::Either::Left(left.borrow()),
            Self::Right(right) => either::Either::Right(right.borrow()),
        }
        .into_list()
    }

    #[inline]
    fn into_list(self) -> impl List
    where
        Self: Sized,
    {
        struct Wrapper<L, R>(either::Either<L, R>);

        impl<L, R> List for Wrapper<L, R>
        where
            L: List,
            R: List,
        {
            #[inline]
            fn len(&self) -> c_uint {
                match &self.0 {
                    either::Either::Left(left) => left.len(),
                    either::Either::Right(right) => right.len(),
                }
            }

            #[inline]
            fn with_value<'ctx, 'eval, T>(
                &self,
                idx: c_uint,
                fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
                ctx: &'ctx mut Context<'eval>,
            ) -> T {
                match &self.0 {
                    either::Either::Left(left) => {
                        left.with_value(idx, fun, ctx)
                    },
                    either::Either::Right(right) => {
                        right.with_value(idx, fun, ctx)
                    },
                }
            }
        }

        Wrapper(self)
    }
}
