//! TODO: docs.

use core::ffi::{CStr, c_uint};
use core::fmt;
use core::ptr::NonNull;
use std::borrow::Cow;
use std::ffi::CString;

pub use nix_bindings_macros::attrset;
use nix_bindings_sys as sys;

use crate::error::{ErrorKind, ToError, TypeMismatchError};
use crate::namespace::{Namespace, PoppableNamespace};
use crate::prelude::{Context, Result, Utf8CStr, Value, ValueKind};
use crate::value::{FnOnceValue, TryFromValue, Values};

/// TODO: docs.
pub trait Attrset: Sized {
    /// Returns an [`Attrset`] implementation that borrows from `self`.
    #[inline]
    fn borrow(&self) -> impl Attrset {
        struct BorrowedAttrset<'a, T> {
            inner: &'a T,
        }

        impl<T: Attrset> Attrset for BorrowedAttrset<'_, T> {
            #[inline]
            fn get_key(&self, idx: c_uint) -> &str {
                self.inner.get_key(idx)
            }

            #[inline]
            fn get_key_as_c_str(&self, idx: c_uint) -> &CStr {
                self.inner.get_key_as_c_str(idx)
            }

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

        BorrowedAttrset { inner: self }
    }

    /// TODO: docs.
    #[inline]
    fn get<T: TryFromValue>(
        &self,
        key: &CStr,
        ctx: &mut Context,
    ) -> Result<T> {
        self.get_opt(key, ctx)?.ok_or_else(|| {
            ctx.make_error(MissingAttributeError {
                attrset: self.borrow(),
                attr: key,
            })
        })
    }

    /// TODO: docs.
    #[inline]
    fn get_opt<T: TryFromValue>(
        &self,
        _key: &CStr,
        _ctx: &mut Context,
    ) -> Result<Option<T>> {
        todo!();
    }

    /// Returns the index of the attribute with the given key, or `None` if no
    /// such key exists.
    ///
    /// If an index is returned, it is guaranteed to be less than `self.len()`.
    #[inline]
    fn get_idx_of_key(&self, key: &str) -> Option<c_uint> {
        (0..self.len()).find(|idx| self.get_key(*idx) == key)
    }

    /// Returns the key of the attribute at the given index.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds (i.e. greater than or equal to
    /// `self.len()`).
    fn get_key(&self, idx: c_uint) -> &str;

    /// Same as [`get_key_by_idx`](Attrset::get_key_by_idx), but returns the
    /// key as a `&CStr`.
    fn get_key_as_c_str(&self, idx: c_uint) -> &CStr;

    /// Returns the [`ValueKind`] of the attribute at the given index.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds (i.e. greater than or equal to
    /// `self.len()`).
    fn get_value_kind(&self, idx: c_uint) -> ValueKind;

    /// Returns the number of attributes in this attribute set.
    fn len(&self) -> c_uint;

    /// TODO: docs.
    #[inline]
    fn into_value(self) -> impl Value {
        AttrsetValue(self)
    }

    /// Returns whether this attribute set is empty.
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
pub struct AnyAttrset {
    inner: NonNull<sys::Value>,
}

/// The attribute set type produced by the [`attrset!`] macro.
pub struct LiteralAttrset<Keys, Values> {
    keys: Keys,
    values: Values,
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

impl<Keys, Values> LiteralAttrset<Keys, Values>
where
    Self: Attrset,
{
    /// Creates a new `LiteralAttrset`.
    #[inline]
    pub fn new(keys: Keys, values: Values) -> Self {
        Self { keys, values }
    }
}

impl Attrset for AnyAttrset {
    #[inline]
    fn get_key(&self, _idx: c_uint) -> &str {
        todo!()
    }

    #[inline]
    fn get_key_as_c_str(&self, _idx: c_uint) -> &CStr {
        todo!()
    }

    #[inline]
    fn get_value_kind(&self, _idx: c_uint) -> ValueKind {
        todo!()
    }

    #[inline]
    fn len(&self) -> c_uint {
        todo!()
    }

    #[inline]
    unsafe fn write_value(
        &self,
        _idx: c_uint,
        _dest: NonNull<nix_bindings_sys::Value>,
        _namespace: impl Namespace,
        _ctx: &mut Context,
    ) -> Result<()> {
        todo!()
    }
}

impl TryFromValue for AnyAttrset {
    #[inline]
    unsafe fn try_from_value(
        value: NonNull<sys::Value>,
        ctx: &mut Context,
    ) -> Result<Self> {
        ctx.force(value)?;

        match ctx.get_kind(value)? {
            ValueKind::Attrset => Ok(Self { inner: value }),
            other => Err(ctx.make_error(TypeMismatchError {
                expected: ValueKind::Attrset,
                found: other,
            })),
        }
    }
}

impl<K: Keys, V: Values> Attrset for LiteralAttrset<K, V> {
    #[inline]
    fn get_key(&self, idx: c_uint) -> &str {
        struct GetKey;
        impl<'a> FnOnceKey<'a, &'a str> for GetKey {
            fn call(self, value: &'a impl AsRef<Utf8CStr>) -> &'a str {
                value.as_ref().as_str()
            }
        }
        self.keys.with_key(idx, GetKey)
    }

    #[inline]
    fn get_key_as_c_str(&self, idx: c_uint) -> &CStr {
        struct GetKeyAsCStr;
        impl<'a> FnOnceKey<'a, &'a CStr> for GetKeyAsCStr {
            fn call(self, value: &'a impl AsRef<Utf8CStr>) -> &'a CStr {
                value.as_ref().as_c_str()
            }
        }
        self.keys.with_key(idx, GetKeyAsCStr)
    }

    #[inline]
    fn get_value_kind(&self, idx: c_uint) -> ValueKind {
        struct GetValueKind;
        impl FnOnceValue<'_, ValueKind> for GetValueKind {
            fn call(self, value: &impl Value) -> ValueKind {
                value.kind()
            }
        }
        self.values.with_value(idx, GetValueKind)
    }

    #[inline]
    fn len(&self) -> c_uint {
        debug_assert_eq!(K::LEN, V::LEN);
        K::LEN
    }

    #[inline]
    unsafe fn write_value(
        &self,
        idx: c_uint,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        struct WriteValue<'ctx, N> {
            dest: NonNull<sys::Value>,
            namespace: N,
            ctx: &'ctx mut Context,
        }
        impl<N: Namespace> FnOnceValue<'_, Result<()>> for WriteValue<'_, N> {
            fn call(self, value: &impl Value) -> Result<()> {
                unsafe {
                    value.write_with_namespace(
                        self.dest,
                        self.namespace,
                        self.ctx,
                    )
                }
            }
        }
        self.values.with_value(idx, WriteValue { dest, namespace, ctx })
    }
}

impl<Keys, Values> Value for LiteralAttrset<Keys, Values>
where
    Self: Attrset,
{
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::Attrset
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
            self.borrow()
                .into_value()
                .write_with_namespace(dest, namespace, ctx)
        }
    }
}

impl<A: Attrset> fmt::Display for MissingAttributeError<'_, A> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "attribute '`{:?}`' missing", self.attr)
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

/// A newtype wrapper that implements `Value` for every `Attrset`.
struct AttrsetValue<T>(T);

impl<T: Attrset> Value for AttrsetValue<T> {
    #[inline]
    fn kind(&self) -> ValueKind {
        ValueKind::Attrset
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
        let Self(attrset) = self;

        unsafe {
            let len = attrset.len();
            let mut builder = ctx.make_attrset_builder(len as usize)?;
            for idx in 0..len {
                let key = attrset.get_key_as_c_str(idx);
                let new_namespace = namespace.push(key);
                builder.insert(key, |dest, ctx| {
                    attrset.write_value(idx, dest, new_namespace, ctx)
                })?;
                namespace = new_namespace.pop();
            }
            builder.build(dest)
        }
    }
}

#[rustfmt::skip]
mod keys_values_impls {
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
