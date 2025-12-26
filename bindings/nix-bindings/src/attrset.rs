//! TODO: docs.

use alloc::borrow::ToOwned;
use alloc::ffi::CString;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::{OnceCell, RefCell};
use core::ffi::{CStr, c_uint};
use core::fmt::{Display, Write};
use core::marker::PhantomData;
use core::ops::{Bound, Deref, Range, RangeBounds};
use core::ptr::NonNull;
use core::result::Result as CoreResult;
use core::{fmt, ptr};

pub use nix_bindings_macros::attrset;
use {nix_bindings_cpp as cpp, nix_bindings_sys as sys};

use crate::context::EvalState;
use crate::error::{Error, ErrorKind, TypeMismatchError};
use crate::namespace::{Namespace, PoppableNamespace};
use crate::prelude::{
    Callable,
    Context,
    NixLambda,
    Result,
    Utf8CStr,
    Value,
    ValueKind,
};
#[cfg(feature = "std")]
use crate::value::ToValue;
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
    ///
    /// # Safety
    ///
    /// The caller must ensure that there are no overlapping keys between
    /// `self` and `other`.
    #[inline(always)]
    unsafe fn concat<T: Attrset>(self, other: T) -> ConcatAttrset<Self, T>
    where
        Self: Sized,
    {
        ConcatAttrset { left: self, right: other }
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
pub trait Key {
    /// TODO: docs.
    fn with_cstr<T>(&self, fun: impl FnOnce(&CStr) -> T) -> T;
}

/// TODO: docs.
pub trait Keys {
    /// TODO: docs.
    const LEN: c_uint;

    /// TODO: docs.
    fn with_key<T>(&self, key_idx: c_uint, fun: impl FnOnceKey<T>) -> T;

    /// TODO: docs.
    #[track_caller]
    fn borrow(&self) -> impl Keys {
        struct Wrapper<'a, K: ?Sized> {
            inner: &'a K,
        }

        impl<K: Keys + ?Sized> Keys for Wrapper<'_, K> {
            const LEN: c_uint = K::LEN;

            #[inline(always)]
            fn with_key<T>(
                &self,
                key_idx: c_uint,
                fun: impl FnOnceKey<T>,
            ) -> T {
                self.inner.with_key(key_idx, fun)
            }
        }

        Wrapper { inner: self }
    }

    /// TODO: docs.
    #[track_caller]
    fn len(&self) -> c_uint {
        Self::LEN
    }

    /// TODO: docs.
    #[track_caller]
    fn slice(&self, range: impl RangeBounds<c_uint>) -> impl Display {
        struct Wrapper<K> {
            keys: K,
            idx_range: Range<c_uint>,
        }

        impl<K: Keys> fmt::Display for Wrapper<K> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                for key_idx in self.idx_range.clone() {
                    self.keys.with_key(key_idx, |key: &CStr| {
                        f.write_str(&*key.to_string_lossy())
                    })?;
                    let is_last = key_idx + 1 == self.idx_range.end;
                    if !is_last {
                        f.write_char('.')?;
                    }
                }
                Ok(())
            }
        }

        let start_idx = match range.start_bound() {
            Bound::Included(idx) => *idx,
            Bound::Excluded(idx) => *idx + 1,
            Bound::Unbounded => 0,
        };

        let end_idx = match range.end_bound() {
            Bound::Included(idx) => *idx + 1,
            Bound::Excluded(idx) => *idx,
            Bound::Unbounded => Self::LEN,
        };

        Wrapper { keys: self.borrow(), idx_range: start_idx..end_idx }
    }
}

/// TODO: docs.
pub trait Pairs {
    /// Advances the iterator to the next key-value pair.
    ///
    /// Note that this method should only be called after
    /// [`is_exhausted()`](Pairs::is_exhausted) returns `false`.
    fn advance(&mut self, context: &mut Context);

    /// Returns `true` if there are no more pairs to iterate over.
    fn is_exhausted(&self) -> bool;

    /// Calls the given function with the key of the current key-value pair.
    ///
    /// Note that this method should only be called after
    /// [`is_exhausted()`](Pairs::is_exhausted) returns `false`.
    fn with_key<T>(&self, fun: impl FnOnceKey<T>) -> T;

    /// Calls the given function with the value of the current key-value pair.
    ///
    /// Note that this method should only be called after
    /// [`is_exhausted()`](Pairs::is_exhausted) returns `false`.
    fn with_value<'ctx, 'eval, T>(
        &self,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> T;
}

/// TODO: docs.
pub trait FnOnceKey<T> {
    /// Calls the function with the given key.
    fn call(self, key: impl Key) -> T;
}

impl<F, T> FnOnceKey<T> for F
where
    F: FnOnce(&CStr) -> T,
{
    #[inline(always)]
    fn call(self, key: impl Key) -> T {
        key.with_cstr(self)
    }
}

/// TODO: docs.
#[derive(Copy, Clone)]
pub struct NixAttrset<'value> {
    inner: NixValue<'value>,
}

/// The attribute set type produced by the [`attrset!`] macro.
#[derive(Clone)]
pub struct LiteralAttrset<Keys, Values> {
    keys: Keys,
    values: Values,
}

/// TODO: docs.
#[derive(Copy, Clone)]
pub struct NixDerivation<'attr> {
    inner: NixAttrset<'attr>,
}

/// The attribute set type created by [`concat`](Attrset::concat)enating two
/// attribute sets.
pub struct ConcatAttrset<Left, Right> {
    left: Left,
    right: Right,
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
pub struct MissingAttributeError<Attrset, Key> {
    /// The attribute set from which the attribute was expected.
    pub attrset: Attrset,

    /// The name of the missing attribute.
    pub key: Key,
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

/// The [`Pairs`] implementation returned by [`Concat::pairs()`].
struct ConcatPairs<Lp, Rp> {
    left_pairs: Lp,
    right_pairs: Rp,
}

/// The [`Pairs`] implementation returned by [`Merge::pairs()`].
struct MergePairs<'a, L, R, Lp, Rp> {
    merge: &'a Merge<L, R>,
    left_pairs: Lp,
    right_pairs: Rp,
    is_current_key_conflicting: bool,
}

/// The [`Pairs`] implementation returned by
/// [`std::collections::HashMap::pairs()`].
#[cfg(feature = "std")]
struct HashMapPairs<'a, K, V> {
    iter: std::collections::hash_map::Iter<'a, K, V>,
    current_pair: Option<(&'a K, &'a V)>,
}

impl<'a> NixAttrset<'a> {
    /// TODO: docs.
    #[inline]
    pub fn get<T: TryFromValue<NixValue<'a>>>(
        self,
        key: impl Keys,
        ctx: &mut Context,
    ) -> Result<T> {
        match self.get_nested(&key, ctx) {
            Ok(value) => Ok(value),
            Err(Ok(idx_of_missing_key)) => Err(MissingAttributeError {
                attrset: self,
                key: key.slice(0..=idx_of_missing_key),
            }
            .into()),
            Err(Err(try_from_value_err)) => Err(try_from_value_err),
        }
    }

    /// TODO: docs.
    #[inline]
    pub fn get_opt<T: TryFromValue<NixValue<'a>>>(
        self,
        key: impl Keys,
        ctx: &mut Context,
    ) -> Result<Option<T>> {
        match self.get_nested(&key, ctx) {
            Ok(value) => Ok(Some(value)),
            Err(Ok(_idx_of_missing_key)) => Ok(None),
            Err(Err(try_from_value_err)) => Err(try_from_value_err),
        }
    }

    /// TODO: docs.
    #[inline]
    pub fn get_single<T: TryFromValue<NixValue<'a>>>(
        self,
        key: &CStr,
        ctx: &mut Context,
    ) -> Result<Option<T>> {
        let Some(value) = self.get_value(key, ctx) else { return Ok(None) };

        T::try_from_value(value, ctx).map(Some).map_err(|err| {
            err.map_message(|msg| {
                let mut orig_msg = msg.into_owned().into_bytes_with_nul();
                let mut new_msg =
                    format!("error getting attribute {key:?}: ").into_bytes();
                new_msg.append(&mut orig_msg);
                // SAFETY: the new message does contain a NUL byte and
                // we've preserved the trailing NUL byte from the
                // original message.
                unsafe { CString::from_vec_with_nul_unchecked(new_msg) }
            })
        })
    }

    #[inline]
    fn get_nested<T: TryFromValue<NixValue<'a>>>(
        self,
        keys: &impl Keys,
        ctx: &mut Context,
    ) -> CoreResult<T, CoreResult<c_uint, Error>> {
        #[inline(always)]
        fn with_key<T>(
            keys: &impl Keys,
            idx: c_uint,
            fun: impl FnOnce(&CStr) -> Result<Option<T>>,
        ) -> CoreResult<T, CoreResult<c_uint, Error>> {
            match keys.with_key(idx, fun) {
                Ok(Some(value)) => Ok(value),
                Ok(None) => Err(Ok(idx)),
                Err(err) => Err(Err(err)),
            }
        }

        let keys_len = keys.len();

        if keys_len == 1 {
            return with_key(keys, 0, |key| self.get_single(key, ctx));
        }

        let mut attrs: NixAttrset =
            with_key(keys, 0, |key| self.get_single(key, ctx))?;

        let mut idx = 1;

        while idx + 1 < keys_len {
            attrs = with_key(keys, idx, |key| attrs.get_single(key, ctx))?;
            idx += 1;
        }

        with_key(keys, idx, |key| attrs.get_single(key, ctx))
    }

    #[inline]
    fn get_value(self, key: &CStr, ctx: &mut Context) -> Option<NixValue<'a>> {
        let value_raw = unsafe {
            cpp::get_attr_byname_lazy(
                self.inner.as_raw(),
                ctx.state_mut().as_ptr(),
                key.as_ptr(),
            )
        };

        NonNull::new(value_raw).map(|ptr| unsafe { NixValue::new(ptr) })
    }
}

impl<K: Keys, V: Values> LiteralAttrset<K, V> {
    /// Creates a new `LiteralAttrset`.
    #[inline]
    pub fn new(keys: K, values: V) -> Self {
        Self { keys, values }
    }
}

impl NixDerivation<'_> {
    /// TODO: docs.
    #[inline]
    pub fn realise(&self, ctx: &mut Context) -> Result<()> {
        let expr = c"drv: \"${drv}\"";
        let string_drv = ctx.eval::<NixLambda>(expr)?.call(self.inner, ctx)?;
        let value = string_drv.into_inner();
        let realised_str = ctx.with_raw_and_state(|ctx, state| unsafe {
            cpp::string_realise(ctx, state.as_ptr(), value.as_raw(), true)
        })?;
        unsafe {
            sys::realised_string_free(realised_str);
        }
        Ok(())
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

        let mut insert = |key: CString| match conflicts.binary_search(&key) {
            Ok(_) => panic!("attrset contains duplicate key {key:?}"),
            Err(idx) => conflicts.insert(idx, key),
        };

        if self.left.len(ctx) <= self.right.len(ctx) {
            let mut left_pairs = self.left.pairs(ctx);

            while !left_pairs.is_exhausted() {
                left_pairs.with_key(|key: &CStr| {
                    if self.right.contains_key(key, ctx) {
                        insert(key.to_owned())
                    }
                });
                left_pairs.advance(ctx);
            }
        } else {
            let mut right_pairs = self.right.pairs(ctx);

            while !right_pairs.is_exhausted() {
                right_pairs.with_key(|key: &CStr| {
                    if self.left.contains_key(key, ctx) {
                        insert(key.to_owned());
                    }
                });
                right_pairs.advance(ctx);
            }
        }

        conflicts
    }
}

impl NixDerivation<'_> {
    /// Returns the output path of this derivation.
    #[cfg(feature = "std")]
    #[inline]
    pub fn out_path(&self, ctx: &mut Context) -> Result<std::path::PathBuf> {
        self.out_path_as_string(ctx).map(Into::into)
    }

    /// Returns the output path of this derivation as a string.
    #[inline]
    pub fn out_path_as_string(&self, ctx: &mut Context) -> Result<String> {
        self.inner.get(c"outPath", ctx)
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
            cpp::attr_iter_create(self.inner.as_raw(), ctx.state_mut().as_ptr())
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
        self.get_value(key, ctx).map(|value| fun.call(value, ctx))
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
            other => Err(TypeMismatchError {
                expected: ValueKind::Attrset,
                found: other,
            }
            .into()),
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
        let idx = (0..K::LEN).find(|&idx| {
            self.keys.with_key(idx, |probe: &CStr| probe == key)
        })?;
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

impl<L: Attrset, R: Attrset> Attrset for ConcatAttrset<L, R> {
    #[inline]
    fn len(&self, ctx: &mut Context) -> c_uint {
        self.left.len(ctx) + self.right.len(ctx)
    }

    #[inline]
    fn pairs<'this, 'eval>(
        &'this self,
        ctx: &mut Context<'eval>,
    ) -> impl Pairs + use<'this, 'eval, L, R> {
        ConcatPairs {
            left_pairs: self.left.pairs(ctx),
            right_pairs: self.right.pairs(ctx),
        }
    }

    #[inline]
    fn with_value<'ctx, 'eval, T>(
        &self,
        key: &CStr,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> Option<T> {
        if self.left.contains_key(key, ctx) {
            self.left.with_value(key, fun, ctx)
        } else {
            self.right.with_value(key, fun, ctx)
        }
    }
}

impl<L, R> Value for ConcatAttrset<L, R>
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
        MergePairs {
            merge: self,
            is_current_key_conflicting: !left_pairs.is_exhausted()
                && left_pairs.with_key(|k: &CStr| self.is_conflicting(k, ctx)),
            left_pairs,
            right_pairs: self.right.pairs(ctx),
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

impl<A: Attrset> Attrset for Option<A> {
    #[inline]
    fn len(&self, ctx: &mut Context) -> c_uint {
        match self {
            Some(attrset) => attrset.len(ctx),
            None => 0,
        }
    }

    #[inline]
    fn pairs<'this, 'eval>(
        &'this self,
        ctx: &mut Context<'eval>,
    ) -> impl Pairs + use<'this, 'eval, A> {
        self.as_ref().map(|attrset| attrset.pairs(ctx))
    }

    #[inline]
    fn with_value<'ctx, 'eval, T>(
        &self,
        key: &CStr,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> Option<T> {
        match self {
            Some(attrset) => attrset.with_value(key, fun, ctx),
            None => None,
        }
    }
}

impl<'a> TryFromValue<NixValue<'a>> for NixDerivation<'a> {
    #[inline]
    fn try_from_value(value: NixValue<'a>, ctx: &mut Context) -> Result<Self> {
        NixAttrset::try_from_value(value, ctx)
            .and_then(|attrset| Self::try_from_value(attrset, ctx))
    }
}

impl<'a> TryFromValue<NixAttrset<'a>> for NixDerivation<'a> {
    #[inline]
    fn try_from_value(
        attrset: NixAttrset<'a>,
        ctx: &mut Context,
    ) -> Result<Self> {
        if attrset.get::<CString>(c"type", ctx)? == c"derivation" {
            Ok(Self { inner: attrset })
        } else {
            Err(Error::new(ErrorKind::Nix, c"not a derivation"))
        }
    }
}

impl Attrset for NixDerivation<'_> {
    #[inline(always)]
    fn len(&self, ctx: &mut Context) -> c_uint {
        self.inner.len(ctx)
    }

    #[inline(always)]
    fn pairs<'this, 'eval>(
        &'this self,
        ctx: &mut Context<'eval>,
    ) -> impl Pairs + use<'this, 'eval> {
        self.inner.pairs(ctx)
    }

    #[inline(always)]
    fn with_value<'ctx, 'eval, T>(
        &self,
        key: &CStr,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> Option<T> {
        Attrset::with_value(&self.inner, key, fun, ctx)
    }
}

impl Value for NixDerivation<'_> {
    #[inline(always)]
    fn kind(&self) -> ValueKind {
        ValueKind::Attrset
    }

    #[inline(always)]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { self.inner.write(dest, namespace, ctx) }
    }
}

impl<'a> Deref for NixDerivation<'a> {
    type Target = NixAttrset<'a>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
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

        impl<N: Namespace> FnOnceValue<Result<()>, &mut Context<'_>> for WriteValue<N> {
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
            pairs.with_key(|key: &CStr| {
                let new_namespace = namespace.push(key);
                builder.insert(key, |dest, ctx| {
                    pairs.with_value(
                        WriteValue { dest, namespace: new_namespace },
                        ctx,
                    )
                })?;
                namespace = new_namespace.pop();
                Ok::<_, Error>(())
            })?;
            pairs.advance(builder.ctx());
        }

        builder.build(dest)
    }
}

impl<T: Key + ?Sized> Key for &T {
    #[inline(always)]
    fn with_cstr<F>(&self, fun: impl FnOnce(&CStr) -> F) -> F {
        (*self).with_cstr(fun)
    }
}

macro_rules! impl_key_for_as_ref_cstr {
    ($ty:ty) => {
        impl Key for $ty {
            #[inline(always)]
            fn with_cstr<T>(&self, fun: impl FnOnce(&CStr) -> T) -> T {
                fun(self.as_ref())
            }
        }
    };
}

impl_key_for_as_ref_cstr!(CStr);
impl_key_for_as_ref_cstr!(Utf8CStr);

/// # Panics
///
/// The [`with_cstr`](Key::with_cstr) implementation will panic if the string
/// contains a NUL byte.
impl Key for str {
    #[track_caller]
    #[inline]
    fn with_cstr<T>(&self, fun: impl FnOnce(&CStr) -> T) -> T {
        thread_local! {
            static KEY_BUFFER: RefCell<Vec<u8>> = RefCell::default();
        }

        if self.as_bytes().contains(&0) {
            panic!(
                "string {self:?} contains a NUL byte, so it cannot be used as \
                 an attrset key"
            )
        }

        KEY_BUFFER.with_borrow_mut(|buf| {
            buf.clear();
            buf.extend_from_slice(self.as_bytes());
            buf.push(0);
            // SAFETY: we checked that the string doesn't contain any NUL bytes,
            // and we just pushed a trailing NUL.
            let cstr = unsafe { CStr::from_bytes_with_nul_unchecked(buf) };
            fun(cstr)
        })
    }
}

impl<K: Key> Keys for K {
    const LEN: c_uint = 1;

    #[inline]
    fn with_key<T>(&self, key_idx: c_uint, fun: impl FnOnceKey<T>) -> T {
        debug_assert!(key_idx == 0);
        fun.call(self)
    }
}

impl<const N: usize, K: Key> Keys for [K; N] {
    const LEN: c_uint = {
        if N > c_uint::MAX as usize {
            panic!("array too long")
        }
        N as c_uint
    };

    #[inline]
    fn with_key<T>(&self, key_idx: c_uint, fun: impl FnOnceKey<T>) -> T {
        fun.call(&self[key_idx as usize])
    }
}

impl Pairs for NixAttrsetPairs<'_, '_> {
    #[inline]
    fn advance(&mut self, _: &mut Context) {
        self.num_attrs_left -= 1;
        unsafe { cpp::attr_iter_advance(self.iterator.as_ptr()) };
    }

    #[inline]
    fn is_exhausted(&self) -> bool {
        self.num_attrs_left == 0
    }

    #[track_caller]
    #[inline]
    fn with_key<T>(&self, fun: impl FnOnceKey<T>) -> T {
        assert!(self.num_attrs_left > 0);
        let key_ptr = unsafe { cpp::attr_iter_key(self.iterator.as_ptr()) };
        // SAFETY: Nix guarantees that the key pointer is valid as long as
        // the iterator is valid.
        let key = unsafe { CStr::from_ptr(key_ptr) };
        fun.call(key)
    }

    #[inline]
    fn with_value<'ctx, 'eval, T>(
        &self,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> T {
        assert!(self.num_attrs_left > 0);

        let value_raw = unsafe { cpp::attr_iter_value(self.iterator.as_ptr()) };

        let value_ptr = NonNull::new(value_raw).expect("value pointer is null");

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
    fn is_exhausted(&self) -> bool {
        self.current_idx == K::LEN
    }

    #[inline]
    fn with_key<T>(&self, fun: impl FnOnceKey<T>) -> T {
        self.attrset.keys.with_key(self.current_idx, fun)
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
        let advance_right_until_we_get_to_non_conflicting_key =
            |this: &mut Self, ctx: &mut Context| {
                while !this.right_pairs.is_exhausted() {
                    let is_conflicting =
                        this.right_pairs.with_key(|key: &CStr| {
                            this.merge.is_conflicting(key, ctx)
                        });

                    if !is_conflicting {
                        break;
                    }

                    this.right_pairs.advance(ctx);
                }
            };

        if self.left_pairs.is_exhausted() {
            self.right_pairs.advance(ctx);
            advance_right_until_we_get_to_non_conflicting_key(self, ctx);
            return;
        }

        self.left_pairs.advance(ctx);

        if self.left_pairs.is_exhausted() {
            advance_right_until_we_get_to_non_conflicting_key(self, ctx);
            self.is_current_key_conflicting = false;
            return;
        }

        self.is_current_key_conflicting = self
            .left_pairs
            .with_key(|key: &CStr| self.merge.is_conflicting(key, ctx));
    }

    #[inline]
    fn is_exhausted(&self) -> bool {
        self.left_pairs.is_exhausted() && self.right_pairs.is_exhausted()
    }

    #[inline]
    fn with_key<T>(&self, fun: impl FnOnceKey<T>) -> T {
        if !self.left_pairs.is_exhausted() {
            self.left_pairs.with_key(fun)
        } else {
            self.right_pairs.with_key(fun)
        }
    }

    #[inline]
    fn with_value<'ctx, 'eval, T>(
        &self,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> T {
        // If we're currently at a conflicting key, use the value from the right
        // attrset.
        if self.is_current_key_conflicting {
            self.left_pairs.with_key(|key: &CStr| {
                self.merge.right.with_value(key, fun, ctx).expect(
                    "key is conflicting, so it must exist in right attrset",
                )
            })
        } else if !self.left_pairs.is_exhausted() {
            self.left_pairs.with_value(fun, ctx)
        } else {
            self.right_pairs.with_value(fun, ctx)
        }
    }
}

impl<Lp, Rp> Pairs for ConcatPairs<Lp, Rp>
where
    Lp: Pairs,
    Rp: Pairs,
{
    #[inline]
    fn advance(&mut self, ctx: &mut Context) {
        if !self.left_pairs.is_exhausted() {
            self.left_pairs.advance(ctx);
        } else {
            self.right_pairs.advance(ctx);
        }
    }

    #[inline]
    fn is_exhausted(&self) -> bool {
        self.left_pairs.is_exhausted() && self.right_pairs.is_exhausted()
    }

    #[inline]
    fn with_key<T>(&self, fun: impl FnOnceKey<T>) -> T {
        if !self.left_pairs.is_exhausted() {
            self.left_pairs.with_key(fun)
        } else {
            self.right_pairs.with_key(fun)
        }
    }

    #[inline]
    fn with_value<'ctx, 'eval, T>(
        &self,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> T {
        if !self.left_pairs.is_exhausted() {
            self.left_pairs.with_value(fun, ctx)
        } else {
            self.right_pairs.with_value(fun, ctx)
        }
    }
}

impl<T: Pairs> Pairs for Option<T> {
    #[inline]
    fn advance(&mut self, ctx: &mut Context) {
        if let Some(pairs) = self {
            pairs.advance(ctx);
        }
    }

    #[inline]
    fn is_exhausted(&self) -> bool {
        match self {
            Some(pairs) => pairs.is_exhausted(),
            None => true,
        }
    }

    #[inline]
    fn with_key<U>(&self, fun: impl FnOnceKey<U>) -> U {
        match self {
            Some(pairs) => pairs.with_key(fun),
            None => panic!("attempted to use key from exhausted pairs"),
        }
    }

    #[inline]
    fn with_value<'ctx, 'eval, U>(
        &self,
        fun: impl FnOnceValue<U, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> U {
        match self {
            Some(pairs) => pairs.with_value(fun, ctx),
            None => panic!("attempted to get value from exhausted pairs"),
        }
    }
}

impl<A: Attrset, K: Display> fmt::Display for MissingAttributeError<A, K> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "attribute at '{}' missing", self.key)
    }
}

impl<A: Attrset, K: Display> From<MissingAttributeError<A, K>> for Error {
    #[inline]
    fn from(err: MissingAttributeError<A, K>) -> Self {
        Self::from_message(err)
    }
}

#[cfg(feature = "compact_str")]
impl Key for compact_str::CompactString {
    #[inline(always)]
    fn with_cstr<T>(&self, fun: impl FnOnce(&CStr) -> T) -> T {
        self.as_str().with_cstr(fun)
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
    fn is_exhausted(&self) -> bool {
        match self {
            Self::Left(l) => l.is_exhausted(),
            Self::Right(r) => r.is_exhausted(),
        }
    }

    #[inline]
    fn with_key<T>(&self, fun: impl FnOnceKey<T>) -> T {
        match self {
            Self::Left(l) => l.with_key(fun),
            Self::Right(r) => r.with_key(fun),
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

#[cfg(feature = "std")]
impl<K, V, S> Attrset for std::collections::HashMap<K, V, S>
where
    K: Eq + core::hash::Hash + core::borrow::Borrow<str>,
    V: ToValue,
    S: core::hash::BuildHasher,
{
    #[inline]
    fn len(&self, _: &mut Context) -> c_uint {
        self.len() as c_uint
    }

    #[inline]
    fn pairs<'this>(
        &'this self,
        _: &mut Context,
    ) -> impl Pairs + use<'this, K, V, S> {
        let mut iter = self.iter();
        HashMapPairs { current_pair: iter.next(), iter }
    }

    #[inline]
    fn with_value<'ctx, 'eval, T>(
        &self,
        key: &CStr,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> Option<T> {
        let key = key.to_str().expect("TODO: make key a Utf8CStr");
        Some(fun.call(self.get(key)?.to_value(ctx), ctx))
    }
}

#[cfg(feature = "std")]
impl<K, V> Pairs for HashMapPairs<'_, K, V>
where
    K: core::borrow::Borrow<str>,
    V: ToValue,
{
    #[inline]
    fn advance(&mut self, _: &mut Context) {
        self.current_pair = self.iter.next();
    }

    #[inline]
    fn is_exhausted(&self) -> bool {
        self.current_pair.is_none()
    }

    #[inline]
    fn with_key<T>(&self, fun: impl FnOnceKey<T>) -> T {
        let Some((key, _value)) = self.current_pair else {
            panic!("attempted to use key from exhausted pairs");
        };
        fun.call(key.borrow())
    }

    #[inline]
    fn with_value<'ctx, 'eval, T>(
        &self,
        fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
        ctx: &'ctx mut Context<'eval>,
    ) -> T {
        let Some((_key, value)) = self.current_pair else {
            panic!("attempted to use value from exhausted pairs");
        };
        fun.call(value.to_value(ctx), ctx)
    }
}

#[cfg(feature = "std")]
impl<K, V, S> Value for std::collections::HashMap<K, V, S>
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
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe {
            Attrset::borrow(self).into_value().write(dest, namespace, ctx)
        }
    }
}

#[rustfmt::skip]
mod keys_tuple_impls {
    use super::*;

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
                $($K: Key),*
            {
                const LEN: c_uint = count!($($K)*);

                #[track_caller]
                #[inline]
                fn with_key<T>(
                    &self,
                    key_idx: c_uint,
                    _fun: impl FnOnceKey<T>,
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

#[doc(hidden)]
pub mod derive {
    //! Contains [`DerivedAttrset`], a trait used in the expansion of the
    //! [`Attrset`](crate::Attrset) derive macro.

    use super::*;

    pub trait DerivedAttrset {
        /// The names of the keys in this attribute set.
        const KEYS: &[&CStr];

        /// The indices of the fields that *might* be skipped.
        const MIGHT_SKIP_IDXS: &[u32];

        /// Returns whether the field at the given index should be skipped.
        fn should_skip(&self, field_idx: u32) -> bool;

        /// Calls the given function with the value of the field at the given
        /// index.
        fn with_value<'ctx, 'eval, T>(
            &self,
            field_idx: c_uint,
            fun: impl FnOnceValue<T, &'ctx mut Context<'eval>>,
            ctx: &'ctx mut Context<'eval>,
        ) -> T;
    }

    struct DerivedAttrsetPairs<'a, T> {
        attrset: &'a T,
        field_idx: u32,
    }

    impl<T: DerivedAttrset> Attrset for T {
        #[inline]
        fn len(&self, _: &mut Context) -> c_uint {
            let mut len = Self::KEYS.len() as c_uint;
            for field_idx in Self::MIGHT_SKIP_IDXS {
                if self.should_skip(*field_idx) {
                    len -= 1;
                }
            }
            len
        }

        #[inline]
        fn pairs<'this>(
            &'this self,
            _: &mut Context,
        ) -> impl Pairs + use<'this, T> {
            let mut field_idx = 0;

            while Self::MIGHT_SKIP_IDXS.get(field_idx as usize)
                == Some(&field_idx)
            {
                if self.should_skip(field_idx) {
                    field_idx += 1;
                } else {
                    break;
                }
            }

            DerivedAttrsetPairs { attrset: self, field_idx }
        }

        #[inline]
        fn with_value<'ctx, 'eval, U>(
            &self,
            key: &CStr,
            fun: impl FnOnceValue<U, &'ctx mut Context<'eval>>,
            ctx: &'ctx mut Context<'eval>,
        ) -> Option<U> {
            let field_idx = Self::KEYS.iter().position(|&k| k == key)? as u32;
            if self.should_skip(field_idx) {
                None
            } else {
                Some(self.with_value(field_idx, fun, ctx))
            }
        }
    }

    impl<T: DerivedAttrset> Pairs for DerivedAttrsetPairs<'_, T> {
        #[inline]
        fn advance(&mut self, _: &mut Context) {
            loop {
                self.field_idx += 1;
                if self.is_exhausted() {
                    break;
                }
                if !self.attrset.should_skip(self.field_idx) {
                    break;
                }
            }
        }

        #[inline]
        fn is_exhausted(&self) -> bool {
            self.field_idx as usize == T::KEYS.len()
        }

        #[inline]
        fn with_key<U>(&self, fun: impl FnOnceKey<U>) -> U {
            fun.call(T::KEYS[self.field_idx as usize])
        }

        #[inline]
        fn with_value<'ctx, 'eval, U>(
            &self,
            fun: impl FnOnceValue<U, &'ctx mut Context<'eval>>,
            ctx: &'ctx mut Context<'eval>,
        ) -> U {
            self.attrset.with_value(self.field_idx, fun, ctx)
        }
    }
}
