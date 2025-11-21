//! TODO: docs.

use core::ffi::CStr;
use core::ptr::NonNull;

use nix_bindings_sys as sys;

use crate::namespace::Namespace;
use crate::prelude::{Context, Result, Utf8CStr, Value, ValueKind};
use crate::value::AttrsetValue;

/// TODO: docs.
pub trait Attrset: Sized {
    /// Returns the index of the attribute with the given key, or `None` if no
    /// such key exists.
    ///
    /// If an index is returned, it is guaranteed to be less than `self.len()`.
    #[inline]
    fn get_idx_of_key(&self, key: &str) -> Option<usize> {
        (0..self.len()).find(|idx| self.get_key(*idx) == key)
    }

    /// Returns the key of the attribute at the given index.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds (i.e. greater than or equal to
    /// `self.len()`).
    fn get_key(&self, idx: usize) -> &str;

    /// Same as [`get_key_by_idx`](Attrset::get_key_by_idx), but returns the
    /// key as a `&CStr`.
    fn get_key_as_c_str(&self, idx: usize) -> &CStr;

    /// Returns the [`ValueKind`] of the attribute at the given index.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds (i.e. greater than or equal to
    /// `self.len()`).
    fn get_value_kind(&self, idx: usize) -> ValueKind;

    /// Returns the number of attributes in this attribute set.
    fn len(&self) -> usize;

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
        idx: usize,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()>;
}

/// TODO: docs.
pub trait Keys {
    /// TODO: docs.
    const LEN: usize;

    /// TODO: docs.
    fn with_key<'a, T: 'a>(
        &'a self,
        key_idx: usize,
        fun: impl FnOnceKey<'a, T>,
    ) -> T;
}

/// TODO: docs.
pub trait Values {
    /// TODO: docs.
    const LEN: usize;

    /// TODO: docs.
    fn with_value<'a, T: 'a>(
        &'a self,
        value_idx: usize,
        fun: impl FnOnceValue<'a, T>,
    ) -> T;
}

/// TODO: docs.
pub trait FnOnceKey<'a, T: 'a> {
    /// TODO: docs.
    fn call(self, value: &'a impl AsRef<Utf8CStr>) -> T;
}

/// TODO: docs.
pub trait FnOnceValue<'a, T: 'a> {
    /// TODO: docs.
    fn call(self, value: &'a impl Value) -> T;
}

/// The attribute set type produced by the [`attrset!`](crate::attrset) macro.
pub struct LiteralAttrset<Keys, Values> {
    keys: Keys,
    values: Values,
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

impl<K: Keys, V: Values> Attrset for LiteralAttrset<K, V> {
    #[inline]
    fn get_key(&self, idx: usize) -> &str {
        struct GetKey;
        impl<'a> FnOnceKey<'a, &'a str> for GetKey {
            fn call(self, value: &'a impl AsRef<Utf8CStr>) -> &'a str {
                value.as_ref().as_str()
            }
        }
        self.keys.with_key(idx, GetKey)
    }

    #[inline]
    fn get_key_as_c_str(&self, idx: usize) -> &CStr {
        struct GetKeyAsCStr;
        impl<'a> FnOnceKey<'a, &'a CStr> for GetKeyAsCStr {
            fn call(self, value: &'a impl AsRef<Utf8CStr>) -> &'a CStr {
                value.as_ref().as_c_str()
            }
        }
        self.keys.with_key(idx, GetKeyAsCStr)
    }

    #[inline]
    fn get_value_kind(&self, idx: usize) -> ValueKind {
        struct GetValueKind;
        impl FnOnceValue<'_, ValueKind> for GetValueKind {
            fn call(self, value: &impl Value) -> ValueKind {
                value.kind()
            }
        }
        self.values.with_value(idx, GetValueKind)
    }

    #[inline]
    fn len(&self) -> usize {
        debug_assert_eq!(K::LEN, V::LEN);
        K::LEN
    }

    #[inline]
    unsafe fn write_value(
        &self,
        idx: usize,
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

impl<T: Attrset> Attrset for &T {
    #[inline]
    fn get_idx_of_key(&self, key: &str) -> Option<usize> {
        (*self).get_idx_of_key(key)
    }

    #[inline]
    fn get_key(&self, idx: usize) -> &str {
        (*self).get_key(idx)
    }

    #[inline]
    fn get_key_as_c_str(&self, idx: usize) -> &CStr {
        (*self).get_key_as_c_str(idx)
    }

    #[inline]
    fn get_value_kind(&self, idx: usize) -> ValueKind {
        (*self).get_value_kind(idx)
    }

    #[inline]
    fn len(&self) -> usize {
        (*self).len()
    }

    #[inline]
    unsafe fn write_value(
        &self,
        idx: usize,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { (*self).write_value(idx, dest, namespace, ctx) }
    }
}

#[inline(never)]
fn panic_tuple_index_oob(idx: usize, len: usize) -> ! {
    panic!("{len}-tuple received out of bounds index: {idx}")
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
                const LEN: usize = count!($($K)*);

                #[track_caller]
                #[inline]
                fn with_key<'a, T: 'a>(
                    &'a self,
                    key_idx: usize,
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
                const LEN: usize = count!($($K)*);

                #[track_caller]
                #[inline]
                fn with_value<'a, T: 'a>(
                    &'a self,
                    value_idx: usize,
                    _fun: impl FnOnceValue<'a, T>,
                ) -> T {
                    match value_idx {
                        $($idx => _fun.call(&self.$idx),)*
                        other => panic_tuple_index_oob(other, <Self as Values>::LEN),
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
}
