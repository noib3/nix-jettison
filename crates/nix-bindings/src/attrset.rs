//! TODO: docs.

use core::ffi::{CStr, c_uint};
use core::ptr::NonNull;
use core::{fmt, ptr};
use std::borrow::Cow;
use std::ffi::CString;

pub use nix_bindings_macros::attrset;
use {nix_bindings_cpp as cpp, nix_bindings_sys as sys};

use crate::error::{ErrorKind, ToError, TypeMismatchError};
use crate::namespace::{Namespace, PoppableNamespace};
use crate::prelude::{Context, Result, Utf8CStr, Value, ValueKind};
use crate::value::{FnOnceValue, NixValue, TryFromValue, Values};

/// TODO: docs.
pub trait Attrset {
    /// Returns an [`Attrset`] implementation that borrows from `self`.
    #[inline]
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
            fn len(&self) -> c_uint {
                self.inner.len()
            }

            #[inline]
            fn with_value<V>(
                &self,
                key: &CStr,
                fun: impl FnOnceValue<V>,
                ctx: &mut Context,
            ) -> Result<Option<V>> {
                self.inner.with_value(key, fun, ctx)
            }
        }

        BorrowedAttrset { inner: self }
    }

    /// Returns the number of attributes in this attribute set.
    fn len(&self) -> c_uint;

    /// Returns whether this attribute set is empty.
    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// TODO: docs.
    fn with_value<T>(
        &self,
        key: &CStr,
        fun: impl FnOnceValue<T>,
        ctx: &mut Context,
    ) -> Result<Option<T>>;
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

/// The type of error returned when an expected attribute is missing from
/// an [`Attrset`].
#[derive(Debug)]
pub struct MissingAttributeError<'a, Attrset> {
    /// The attribute set from which the attribute was expected.
    pub attrset: Attrset,

    /// The name of the missing attribute.
    pub attr: &'a CStr,
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
                attrset: self.borrow(),
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
    fn with_attr_inner<T: 'a>(
        self,
        key: &CStr,
        fun: impl FnOnce(NixValue<'a>, &mut Context) -> T,
        ctx: &mut Context,
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

    /// Returns the index of the attribute with the given key, or `None` if no
    /// such key exists.
    ///
    /// If an index is returned, it is guaranteed to be less than `self.len()`.
    #[inline]
    fn get_idx_of_key(&self, key: &CStr) -> Option<c_uint> {
        (0..self.len()).find(|idx| self.get_key(*idx) == key)
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

impl Attrset for NixAttrset<'_> {
    #[inline]
    fn len(&self) -> c_uint {
        // 'nix_get_attrs_size' errors when the value pointer is null or when
        // the value is not initizialized, but having a ValuePointer guarantees
        // neither of those can happen, so we can use a null context.
        unsafe { sys::get_attrs_size(ptr::null_mut(), self.inner.as_raw()) }
    }

    #[inline]
    fn with_value<T>(
        &self,
        key: &CStr,
        fun: impl FnOnceValue<T>,
        ctx: &mut Context,
    ) -> Result<Option<T>> {
        Ok(self.with_attr_inner(key, |value, _| fun.call(value), ctx))
    }
}

impl<'a> TryFromValue<NixValue<'a>> for NixAttrset<'a> {
    #[inline]
    fn try_from_value(value: NixValue<'a>, ctx: &mut Context) -> Result<Self> {
        ctx.force(value.as_ptr())?;

        match value.kind() {
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
    fn len(&self) -> c_uint {
        debug_assert_eq!(K::LEN, V::LEN);
        K::LEN
    }

    #[inline]
    fn with_value<T>(
        &self,
        key: &CStr,
        fun: impl FnOnceValue<T>,
        _: &mut Context,
    ) -> Result<Option<T>> {
        Ok(self
            .get_idx_of_key(key)
            .map(|idx| self.values.with_value(idx, fun)))
    }
}

impl<K: Keys, V: Values> Value for LiteralAttrset<K, V> {
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

        let mut builder = ctx.make_attrset_builder(len as usize)?;

        for idx in 0..len {
            let key = self.get_key(idx);
            let new_namespace = namespace.push(key);
            builder.insert(key, |dest, ctx| {
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
