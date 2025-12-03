//! TODO: docs.

use alloc::borrow::Cow;
use alloc::ffi::CString;
use alloc::vec::Vec;
use core::cell::OnceCell;
use core::ffi::{CStr, c_uint};
use core::ptr::NonNull;
use core::{fmt, ptr};

pub use nix_bindings_macros::attrset;
use {nix_bindings_cpp as cpp, nix_bindings_sys as sys};

use crate::context::AttrsetBuilder;
use crate::error::{ErrorKind, ToError, TypeMismatchError};
use crate::namespace::{Namespace, PoppableNamespace};
use crate::prelude::{Context, Result, Utf8CStr, Value, ValueKind};
use crate::value::{FnOnceValue, NixValue, TryFromValue, Values};

/// TODO: docs.
pub trait Attrset {
    /// Returns the number of attributes in this attribute set.
    fn len(&self, ctx: &mut Context) -> c_uint;

    /// Returns the number of attributes in this attribute set.
    fn pairs(&self) -> impl Pairs;

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
            fn pairs(&self) -> impl Pairs {
                self.inner.pairs()
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
    fn with_next_key_value<'this, T: 'this, Ctx>(
        &'this mut self,
        fun: impl FnOnceValue<T, (&'this CStr, Ctx)>,
        ctx: Ctx,
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

/// A newtype wrapper that implements [`Value`] for every [`Attrset`].
struct MergePairs<'a, M> {
    _merge: &'a M,
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

    #[inline]
    fn init_conflicts(&self, ctx: &mut Context) -> Vec<CString> {
        struct PushConflict<'a, 'b, S> {
            conflicts: &'a mut Vec<CString>,
            set: &'b S,
        }

        impl<S: Attrset> FnOnceValue<(), (&CStr, &mut Context<'_>)>
            for PushConflict<'_, '_, S>
        {
            #[inline]
            fn call(
                self,
                _: impl Value,
                (key, ctx): (&CStr, &mut Context<'_>),
            ) {
                if self.set.contains_key(key, ctx) {
                    self.conflicts.push(key.to_owned());
                }
            }
        }

        let mut conflicts = Vec::new();

        let left_len = self.left.len(ctx);
        let right_len = self.right.len(ctx);

        if left_len <= right_len {
            let mut left_pairs = self.left.pairs();

            for _ in 0..left_len {
                left_pairs.with_next_key_value(
                    PushConflict {
                        conflicts: &mut conflicts,
                        set: &self.right,
                    },
                    ctx,
                );
            }
        } else {
            let mut right_pairs = self.right.pairs();

            for _ in 0..right_len {
                right_pairs.with_next_key_value(
                    PushConflict {
                        conflicts: &mut conflicts,
                        set: &self.left,
                    },
                    ctx,
                );
            }
        }

        conflicts
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
    fn pairs(&self) -> impl Pairs {
        TodoPairs
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
    fn pairs(&self) -> impl Pairs {
        TodoPairs
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
    fn pairs(&self) -> impl Pairs {
        MergePairs { _merge: self }
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
        struct InsertPair<'a, 'ctx, 'eval, 'ns, N> {
            builder: &'a mut AttrsetBuilder<'ctx, 'eval>,
            namespace: &'ns mut N,
        }

        impl<N: Namespace> FnOnceValue<Result<()>, (&CStr, ())>
            for InsertPair<'_, '_, '_, '_, N>
        {
            #[inline]
            fn call(
                self,
                value: impl Value,
                (key, ()): (&CStr, ()),
            ) -> Result<()> {
                let new_namespace = self.namespace.push(key);
                self.builder.insert(key, |dest, ctx| unsafe {
                    value.write(dest, new_namespace, ctx)
                })?;
                *self.namespace = new_namespace.pop();
                Ok(())
            }
        }

        let Self(attrset) = self;

        let len = attrset.len(ctx);

        let mut pairs = attrset.pairs();

        let mut builder = ctx.make_attrset_builder(len as usize)?;

        for _ in 0..len {
            pairs.with_next_key_value(
                InsertPair {
                    builder: &mut builder,
                    namespace: &mut namespace,
                },
                (),
            )?;
        }

        builder.build(dest)
    }
}

struct TodoPairs;

impl Pairs for TodoPairs {
    #[inline]
    fn with_next_key_value<'this, T: 'this, Ctx>(
        &'this mut self,
        _fun: impl FnOnceValue<T, (&'this CStr, Ctx)>,
        _ctx: Ctx,
    ) -> T {
        todo!();
    }
}

impl<L: Attrset, R: Attrset> Pairs for MergePairs<'_, Merge<L, R>> {
    #[inline]
    fn with_next_key_value<'this, T: 'this, Ctx>(
        &'this mut self,
        _fun: impl FnOnceValue<T, (&'this CStr, Ctx)>,
        _ctx: Ctx,
    ) -> T {
        todo!();
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
    fn pairs(&self) -> impl Pairs {
        match self {
            Self::Left(l) => either::Either::Left(l.pairs()),
            Self::Right(r) => either::Either::Right(r.pairs()),
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
    fn with_next_key_value<'this, T: 'this, Ctx>(
        &'this mut self,
        fun: impl FnOnceValue<T, (&'this CStr, Ctx)>,
        ctx: Ctx,
    ) -> T {
        match self {
            Self::Left(l) => l.with_next_key_value(fun, ctx),
            Self::Right(r) => r.with_next_key_value(fun, ctx),
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

    #[inline(never)]
    fn panic_tuple_index_oob(idx: c_uint, len: c_uint) -> ! {
        panic!("{len}-tuple received out of bounds index: {idx}")
    }
}
