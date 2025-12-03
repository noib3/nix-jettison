//! TODO: docs.

use alloc::borrow::Cow;
use alloc::ffi::CString;
use alloc::vec::Vec;
use core::cell::OnceCell;
use core::ffi::{CStr, c_uint};
use core::marker::PhantomData;
use core::ptr::NonNull;
use core::{fmt, ptr};

pub use nix_bindings_macros::attrset;
use {nix_bindings_cpp as cpp, nix_bindings_sys as sys};

use crate::context::EvalState;
use crate::error::{ErrorKind, ToError, TypeMismatchError};
use crate::namespace::{Namespace, PoppableNamespace};
use crate::prelude::{Context, Result, Utf8CStr, Value, ValueKind};
use crate::value::{FnOnceValue, NixValue, TryFromValue, Values};

/// TODO: docs.
pub trait Attrset {
    /// Returns the number of attributes in this attribute set.
    fn len(&self, ctx: &mut Context) -> c_uint;

    /// Returns a [`Pairs`] implementation that can be used to iterate
    /// over the key-value pairs in this attribute set.
    fn pairs<'this, 'eval>(
        &'this self,
        ctx: &mut Context<'eval>,
    ) -> impl Pairs + use<'this, 'eval, Self>;

    /// TODO: docs.
    fn with_value<'ctx, 'eval, T>(
        &self,
        key: &CStr,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> Option<T>;

    /// Returns an [`Attrset`] implementation that borrows from `self`.
    #[inline(always)]
    fn borrow(&self) -> impl Attrset {
        struct BorrowedAttrset<'a, T: ?Sized> {
            inner: &'a T,
        }

        impl<T: Attrset + ?Sized> Attrset for BorrowedAttrset<'_, T> {
            #[inline]
            fn borrow(&self) -> impl Attrset {
                Self { inner: self.inner }
            }

            #[inline]
            fn len(&self, ctx: &mut Context) -> c_uint {
                self.inner.len(ctx)
            }

            #[inline]
            fn pairs<'this, 'eval>(
                &'this self,
                ctx: &mut Context<'eval>,
            ) -> impl Pairs + use<'this, 'eval, T> {
                self.inner.pairs(ctx)
            }

            #[inline]
            fn with_value<'ctx, 'eval, U>(
                &self,
                key: &CStr,
                fun: impl FnOnceValue<U, &'ctx mut Context<'eval>>,
                ctx: &'ctx mut Context<'eval>,
            ) -> Option<U> {
                self.inner.with_value(key, fun, ctx)
            }
        }

        BorrowedAttrset { inner: self }
    }

    /// TODO: docs.
    #[inline(always)]
    fn contains_key(&self, key: &CStr, ctx: &mut Context) -> bool {
        struct NoOp;
        impl FnOnceValue<(), &mut Context<'_>> for NoOp {
            fn call(self, _: impl Value, _: &mut Context) {}
        }
        self.with_value(key, NoOp, ctx).is_some()
    }

    /// TODO: docs.
    #[inline(always)]
    fn into_attrset(self) -> impl Attrset
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
        AttrsetValue(self)
    }

    /// Returns whether this attribute set is empty.
    #[inline(always)]
    fn is_empty(&self, ctx: &mut Context) -> bool {
        self.len(ctx) == 0
    }

    /// TODO: docs.
    #[inline(always)]
    fn merge<T: Attrset>(self, other: T) -> Merge<Self, T>
    where
        Self: Sized,
    {
        Merge { left: self, right: other, conflicts: OnceCell::new() }
    }
}

/// TODO: docs.
pub trait Pairs {
    /// TODO: docs.
    fn advance(&mut self, context: &mut Context);

    /// TODO: docs.
    fn key(&self, ctx: &mut Context) -> &CStr;

    /// TODO: docs.
    fn with_value<'ctx, 'eval, T>(
        &self,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> T;
}

/// TODO: docs.
pub trait Keys {
    /// TODO: docs.
    const LEN: c_uint;

    /// TODO: docs.
    fn with_key<'a, T: 'a>(
        &'a self,
        key_idx: c_uint,
        fun: impl FnOnceKey<'a, T>,
    ) -> T;
}

/// TODO: docs.
pub trait FnOnceKey<'a, T: 'a> {
    /// TODO: docs.
    fn call(self, value: &'a impl AsRef<Utf8CStr>) -> T;
}

/// TODO: docs.
#[derive(Copy, Clone)]
pub struct NixAttrset<'value> {
    inner: NixValue<'value>,
}

/// The attribute set type produced by the [`attrset!`] macro.
pub struct LiteralAttrset<Keys, Values> {
    keys: Keys,
    values: Values,
}

/// The attribute set type created by [`merge`](Attrset::merge)ing two
/// attribute sets.
pub struct Merge<Left, Right> {
    left: Left,
    right: Right,
    /// The conflicting keys between `left` and `right`, sorted in ascending
    /// order.
    conflicts: OnceCell<Vec<CString>>,
}

/// The type of error returned when an expected attribute is missing from
/// an [`Attrset`].
#[derive(Debug)]
pub struct MissingAttributeError<'a, Attrset> {
    /// The attribute set from which the attribute was expected.
    pub attrset: Attrset,

    /// The name of the missing attribute.
    pub attr: &'a CStr,
}

/// A newtype wrapper that implements [`Value`] for every [`Attrset`].
struct AttrsetValue<T>(T);

/// The [`Pairs`] implementation returned by [`NixAttrset::pairs()`].
struct NixAttrsetPairs<'set, 'eval> {
    iterator: NonNull<cpp::AttrIterator>,
    num_attrs_left: c_uint,
    _lifetimes: PhantomData<(NixAttrset<'set>, EvalState<'eval>)>,
}

/// The [`Pairs`] implementation returned by [`LiteralAttrset::pairs()`].
struct LiteralAttrsetPairs<'a, K, V> {
    attrset: &'a LiteralAttrset<K, V>,
    current_idx: c_uint,
}

/// The [`Pairs`] implementation returned by [`Merge::pairs()`].
struct MergePairs<'a, L, R, Lp, Rp> {
    merge: &'a Merge<L, R>,
    left_pairs: Lp,
    right_pairs: Rp,
    is_current_key_conflicting: bool,
    num_advanced_left: c_uint,
    left_len: c_uint,
}

impl<'a> NixAttrset<'a> {
    /// TODO: docs.
    #[inline]
    pub fn get<T: TryFromValue<NixValue<'a>> + 'a>(
        self,
        key: &CStr,
        ctx: &mut Context,
    ) -> Result<T> {
        self.get_opt(key, ctx)?.ok_or_else(|| {
            ctx.make_error(MissingAttributeError {
                attrset: self.into_attrset().borrow(),
                attr: key,
            })
        })
    }

    /// TODO: docs.
    #[inline]
    pub fn get_opt<T: TryFromValue<NixValue<'a>> + 'a>(
        self,
        key: &CStr,
        ctx: &mut Context,
    ) -> Result<Option<T>> {
        self.with_attr_inner(
            key,
            |value, ctx| T::try_from_value(value, ctx),
            ctx,
        )
        .transpose()
    }

    #[inline]
    fn with_attr_inner<'ctx, 'eval, T: 'a>(
        self,
        key: &CStr,
        fun: impl FnOnce(NixValue<'a>, &'ctx mut Context<'eval>) -> T,
        ctx: &'ctx mut Context<'eval>,
    ) -> Option<T> {
        let value_raw = unsafe {
            cpp::get_attr_byname_lazy(
                self.inner.as_raw(),
                ctx.state_mut().as_ptr(),
                key.as_ptr(),
            )
        };

        let value_ptr = NonNull::new(value_raw)?;

        // SAFETY: the value returned by Nix is initialized.
        Some(fun(unsafe { NixValue::new(value_ptr) }, ctx))
    }
}

impl<K: Keys, V: Values> LiteralAttrset<K, V> {
    /// Creates a new `LiteralAttrset`.
    #[inline]
    pub fn new(keys: K, values: V) -> Self {
        Self { keys, values }
    }

    #[inline]
    fn get_key(&self, idx: c_uint) -> &CStr {
        struct GetKey;
        impl<'a> FnOnceKey<'a, &'a CStr> for GetKey {
            fn call(self, value: &'a impl AsRef<Utf8CStr>) -> &'a CStr {
                value.as_ref().as_c_str()
            }
        }
        self.keys.with_key(idx, GetKey)
    }
}

impl<L: Attrset, R: Attrset> Merge<L, R> {
    #[inline]
    fn conflicts(&self, ctx: &mut Context) -> &[CString] {
        self.conflicts.get_or_init(|| self.init_conflicts(ctx))
    }

    /// Returns whether the given key is conflicting.
    #[inline]
    fn is_conflicting(&self, key: &CStr, ctx: &mut Context) -> bool {
        let conflicts = self.conflicts(ctx);
        conflicts.binary_search_by_key(&key, |c| &**c).is_ok()
    }

    #[inline]
    fn init_conflicts(&self, ctx: &mut Context) -> Vec<CString> {
        let mut conflicts = Vec::new();

        let left_len = self.left.len(ctx);
        let right_len = self.right.len(ctx);

        if left_len <= right_len {
            let mut left_pairs = self.left.pairs(ctx);

            for _ in 0..left_len {
                let key = left_pairs.key(ctx);
                if self.right.contains_key(key, ctx) {
                    conflicts.push(key.to_owned());
                }
                left_pairs.advance(ctx);
            }
        } else {
            let mut right_pairs = self.right.pairs(ctx);

            for _ in 0..right_len {
                let key = right_pairs.key(ctx);
                if self.left.contains_key(key, ctx) {
                    conflicts.push(key.to_owned());
                }
                right_pairs.advance(ctx);
            }
        }

        conflicts
    }
}

impl<'a, L, R, Lp, Rp> MergePairs<'a, L, R, Lp, Rp> {
    #[inline]
    fn is_left_exhausted(&self) -> bool {
        self.num_advanced_left == self.left_len
    }
}

impl Attrset for NixAttrset<'_> {
    #[inline]
    fn into_value(self) -> impl Value
    where
        Self: Sized,
    {
        self
    }

    #[inline]
    fn len(&self, _: &mut Context) -> c_uint {
        // 'nix_get_attrs_size' errors when the value pointer is null or when
        // the value is not initizialized, but having a NixValue guarantees
        // neither of those can happen, so we can use a null context.
        unsafe { sys::get_attrs_size(ptr::null_mut(), self.inner.as_raw()) }
    }

    #[inline]
    fn pairs<'this, 'eval>(
        &'this self,
        ctx: &mut Context<'eval>,
    ) -> impl Pairs + use<'this, 'eval> {
        let iter_raw = unsafe {
            cpp::attr_iter_create(
                self.inner.as_raw(),
                ctx.state_mut().as_ptr(),
            )
        };

        let iterator =
            NonNull::new(iter_raw).expect("failed to create attr iterator");

        NixAttrsetPairs::<'this, 'eval> {
            iterator,
            num_attrs_left: self.len(ctx),
            _lifetimes: PhantomData,
        }
    }

    #[inline]
    fn with_value<'ctx, 'eval, T>(
        &self,
        key: &CStr,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> Option<T> {
        self.with_attr_inner(key, |value, ctx| fun.call(value, ctx), ctx)
    }
}

impl Value for NixAttrset<'_> {
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

impl<'a> TryFromValue<NixValue<'a>> for NixAttrset<'a> {
    #[inline]
    fn try_from_value(
        mut value: NixValue<'a>,
        ctx: &mut Context,
    ) -> Result<Self> {
        value.force_inline(ctx)?;

        match value.kind() {
            ValueKind::Attrset => Ok(Self { inner: value }),
            other => Err(ctx.make_error(TypeMismatchError {
                expected: ValueKind::Attrset,
                found: other,
            })),
        }
    }
}

impl<'a> From<NixAttrset<'a>> for NixValue<'a> {
    #[inline]
    fn from(attrset: NixAttrset<'a>) -> Self {
        attrset.inner
    }
}

impl<K: Keys, V: Values> Attrset for LiteralAttrset<K, V> {
    #[inline]
    fn len(&self, _: &mut Context) -> c_uint {
        debug_assert_eq!(K::LEN, V::LEN);
        K::LEN
    }

    #[inline]
    fn pairs<'this>(
        &'this self,
        _: &mut Context,
    ) -> impl Pairs + use<'this, K, V> {
        LiteralAttrsetPairs { attrset: self, current_idx: 0 }
    }

    #[inline]
    fn with_value<'ctx, 'eval, T>(
        &self,
        key: &CStr,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> Option<T> {
        let idx = (0..K::LEN).find(|&idx| self.get_key(idx) == key)?;
        Some(self.values.with_value(idx, fun.map_ctx(move |()| ctx)))
    }
}

impl<K: Keys, V: Values> Value for LiteralAttrset<K, V> {
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
        unsafe {
            Attrset::borrow(self).into_value().write(dest, namespace, ctx)
        }
    }
}

impl<L: Attrset, R: Attrset> Attrset for Merge<L, R> {
    #[inline]
    fn len(&self, ctx: &mut Context) -> c_uint {
        self.left.len(ctx) + self.right.len(ctx)
            - self.conflicts(ctx).len() as c_uint
    }

    #[inline]
    fn pairs<'this, 'eval>(
        &'this self,
        ctx: &mut Context<'eval>,
    ) -> impl Pairs + use<'this, 'eval, L, R> {
        let left_pairs = self.left.pairs(ctx);
        let left_len = self.left.len(ctx);
        MergePairs {
            merge: self,
            is_current_key_conflicting: left_len > 0
                && self.is_conflicting(left_pairs.key(ctx), ctx),
            left_pairs,
            right_pairs: self.right.pairs(ctx),
            num_advanced_left: 0,
            left_len,
        }
    }

    #[inline]
    fn with_value<'ctx, 'eval, T>(
        &self,
        key: &CStr,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> Option<T> {
        if self.right.contains_key(key, ctx) {
            self.right.with_value(key, fun, ctx)
        } else {
            self.left.with_value(key, fun, ctx)
        }
    }
}

impl<L, R> Value for Merge<L, R>
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
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe {
            Attrset::borrow(self).into_value().write(dest, namespace, ctx)
        }
    }
}

impl<T: Attrset> Value for AttrsetValue<T> {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::Attrset
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

        impl<N: Namespace> FnOnceValue<Result<()>, &mut Context<'_>>
            for WriteValue<N>
        {
            #[inline]
            fn call(self, value: impl Value, ctx: &mut Context) -> Result<()> {
                unsafe { value.write(self.dest, self.namespace, ctx) }
            }
        }

        let Self(attrset) = self;
        let len = attrset.len(ctx);

        let mut pairs = attrset.pairs(ctx);
        let mut builder = ctx.make_attrset_builder(len as usize)?;

        for _ in 0..len {
            let key = pairs.key(builder.ctx());
            let new_namespace = namespace.push(key);
            builder.insert(key, |dest, ctx| {
                pairs.with_value(
                    WriteValue { dest, namespace: new_namespace },
                    ctx,
                )
            })?;
            namespace = new_namespace.pop();
            pairs.advance(builder.ctx());
        }

        builder.build(dest)
    }
}

impl Pairs for NixAttrsetPairs<'_, '_> {
    #[inline]
    fn advance(&mut self, _: &mut Context) {
        self.num_attrs_left -= 1;
        unsafe { cpp::attr_iter_advance(self.iterator.as_ptr()) };
    }

    #[track_caller]
    #[inline]
    fn key(&self, _: &mut Context) -> &CStr {
        assert!(self.num_attrs_left > 0);
        let key_ptr = unsafe { cpp::attr_iter_key(self.iterator.as_ptr()) };
        // SAFETY: Nix guarantees that the key pointer is valid as long as
        // the iterator is valid.
        unsafe { CStr::from_ptr(key_ptr) }
    }

    #[inline]
    fn with_value<'ctx, 'eval, T>(
        &self,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> T {
        assert!(self.num_attrs_left > 0);

        let value_raw =
            unsafe { cpp::attr_iter_value(self.iterator.as_ptr()) };

        let value_ptr =
            NonNull::new(value_raw).expect("value pointer is null");

        // SAFETY: the value returned by Nix is initialized.
        let value = unsafe { NixValue::new(value_ptr) };

        fun.call(value, ctx)
    }
}

impl Drop for NixAttrsetPairs<'_, '_> {
    #[inline]
    fn drop(&mut self) {
        unsafe { cpp::attr_iter_destroy(self.iterator.as_ptr()) };
    }
}

impl<K, V> Pairs for LiteralAttrsetPairs<'_, K, V>
where
    K: Keys,
    V: Values,
{
    #[inline]
    fn advance(&mut self, _: &mut Context) {
        self.current_idx += 1;
    }

    #[inline]
    fn key(&self, _: &mut Context) -> &CStr {
        self.attrset.get_key(self.current_idx)
    }

    #[inline]
    fn with_value<'ctx, 'eval, T>(
        &self,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> T {
        self.attrset
            .values
            .with_value(self.current_idx, fun.map_ctx(move |()| ctx))
    }
}

impl<L, R, Lp, Rp> Pairs for MergePairs<'_, L, R, Lp, Rp>
where
    L: Attrset,
    R: Attrset,
    Lp: Pairs,
    Rp: Pairs,
{
    #[inline]
    fn advance(&mut self, ctx: &mut Context) {
        if self.is_left_exhausted() {
            // Skip all the conflicting keys in the right attrset since they've
            // already been used while iterating over the left attrset.
            loop {
                self.right_pairs.advance(ctx);
                let key = self.right_pairs.key(ctx);
                if !self.merge.is_conflicting(key, ctx) {
                    return;
                }
            }
        }

        self.left_pairs.advance(ctx);
        let key = self.left_pairs.key(ctx);
        self.is_current_key_conflicting = self.merge.is_conflicting(key, ctx);
        self.num_advanced_left += 1;
    }

    #[inline]
    fn key(&self, ctx: &mut Context) -> &CStr {
        if !self.is_left_exhausted() {
            self.left_pairs.key(ctx)
        } else {
            self.right_pairs.key(ctx)
        }
    }

    #[inline]
    fn with_value<'ctx, 'eval, T>(
        &self,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> T {
        // If we're currently at a conflicting key, use the value from the
        // right attrset.
        if self.is_current_key_conflicting {
            let key = self.left_pairs.key(ctx);
            let out = self.merge.right.with_value(key, fun, ctx);
            out.expect("key is conflicting, so it must exist in right attrset")
        } else if !self.is_left_exhausted() {
            self.left_pairs.with_value(fun, ctx)
        } else {
            self.right_pairs.with_value(fun, ctx)
        }
    }
}

impl<A: Attrset> fmt::Display for MissingAttributeError<'_, A> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "attribute '{}' missing", self.attr.to_string_lossy())
    }
}

impl<A: Attrset> ToError for MissingAttributeError<'_, A> {
    #[inline]
    fn kind(&self) -> ErrorKind {
        ErrorKind::Nix
    }

    #[inline]
    fn format_to_c_str(&self) -> Cow<'_, CStr> {
        // SAFETY: the Display impl doesn't contain any NUL bytes.
        unsafe { CString::from_vec_unchecked(self.to_string().into()).into() }
    }
}

#[cfg(feature = "either")]
impl<L: Attrset, R: Attrset> Attrset for either::Either<L, R> {
    #[inline]
    fn len(&self, ctx: &mut Context) -> c_uint {
        match self {
            Self::Left(l) => l.len(ctx),
            Self::Right(r) => r.len(ctx),
        }
    }

    #[inline]
    fn pairs<'this, 'eval>(
        &'this self,
        ctx: &mut Context<'eval>,
    ) -> impl Pairs + use<'this, 'eval, L, R> {
        match self {
            Self::Left(l) => either::Either::Left(l.pairs(ctx)),
            Self::Right(r) => either::Either::Right(r.pairs(ctx)),
        }
    }

    #[inline]
    fn with_value<'ctx, 'eval, T>(
        &self,
        key: &CStr,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> Option<T> {
        match self {
            Self::Left(l) => l.with_value(key, fun, ctx),
            Self::Right(r) => r.with_value(key, fun, ctx),
        }
    }
}

#[cfg(feature = "either")]
impl<L: Pairs, R: Pairs> Pairs for either::Either<L, R> {
    #[inline]
    fn advance(&mut self, ctx: &mut Context) {
        match self {
            Self::Left(l) => l.advance(ctx),
            Self::Right(r) => r.advance(ctx),
        }
    }

    #[inline]
    fn key(&self, ctx: &mut Context) -> &CStr {
        match self {
            Self::Left(l) => l.key(ctx),
            Self::Right(r) => r.key(ctx),
        }
    }

    #[inline]
    fn with_value<'ctx, 'eval, T>(
        &self,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> T {
        match self {
            Self::Left(l) => l.with_value(fun, ctx),
            Self::Right(r) => r.with_value(fun, ctx),
        }
    }
}

#[rustfmt::skip]
mod keys_impls {
    use super::*;

    impl<Key: AsRef<Utf8CStr>> Keys for Key {
        const LEN: c_uint = 1;

        #[track_caller]
        #[inline]
        fn with_key<'a, T: 'a>(
            &'a self,
            key_idx: c_uint,
            fun: impl FnOnceKey<'a, T>,
        ) -> T {
            match key_idx {
                0 => fun.call(self),
                other => panic_tuple_index_oob(other, <Self as Keys>::LEN),
            }
        }
    }

    macro_rules! count {
        () => { 0 };
        ($x:tt $($xs:tt)*) => { 1 + count!($($xs)*) };
    }

    macro_rules! impl_keys {
        ($($K:ident),*) => {
            impl_keys!(@pair [] [0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31] [$($K)*]);
        };

        (@pair [$($pairs:tt)*] [$next_idx:tt $($rest_idx:tt)*] [$next_K:ident $($rest_K:ident)*]) => {
            impl_keys!(@pair [$($pairs)* ($next_idx $next_K)] [$($rest_idx)*] [$($rest_K)*]);
        };

        (@pair [$(($idx:tt $K:ident))*] $_:tt []) => {
            impl<$($K),*> Keys for ($($K,)*)
            where
                $($K: AsRef<Utf8CStr>),*
            {
                const LEN: c_uint = count!($($K)*);

                #[track_caller]
                #[inline]
                fn with_key<'a, T: 'a>(
                    &'a self,
                    key_idx: c_uint,
                    _fun: impl FnOnceKey<'a, T>,
                ) -> T {
                    match key_idx {
                        $($idx => _fun.call(&self.$idx),)*
                        other => panic_tuple_index_oob(other, <Self as Keys>::LEN),
                    }
                }
            }
        };
    }

    impl_keys!();
    impl_keys!(K);
    impl_keys!(K1, K2);
    impl_keys!(K1, K2, K3);
    impl_keys!(K1, K2, K3, K4);
    impl_keys!(K1, K2, K3, K4, K5);
    impl_keys!(K1, K2, K3, K4, K5, K6);
    impl_keys!(K1, K2, K3, K4, K5, K6, K7);
    impl_keys!(K1, K2, K3, K4, K5, K6, K7, K8);
    impl_keys!(K1, K2, K3, K4, K5, K6, K7, K8, K9);
    impl_keys!(K1, K2, K3, K4, K5, K6, K7, K8, K9, K10);
    impl_keys!(K1, K2, K3, K4, K5, K6, K7, K8, K9, K10, K11);
    impl_keys!(K1, K2, K3, K4, K5, K6, K7, K8, K9, K10, K11, K12);
    impl_keys!(K1, K2, K3, K4, K5, K6, K7, K8, K9, K10, K11, K12, K13);
    impl_keys!(K1, K2, K3, K4, K5, K6, K7, K8, K9, K10, K11, K12, K13, K14);
    impl_keys!(K1, K2, K3, K4, K5, K6, K7, K8, K9, K10, K11, K12, K13, K14, K15);
    impl_keys!(K1, K2, K3, K4, K5, K6, K7, K8, K9, K10, K11, K12, K13, K14, K15, K16);

    #[track_caller]
    #[inline(never)]
    fn panic_tuple_index_oob(idx: c_uint, len: c_uint) -> ! {
        panic!("{len}-tuple received out of bounds index: {idx}")
    }
}
