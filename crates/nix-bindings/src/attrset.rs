use core::ffi::CStr;
use core::ptr::NonNull;

use nix_bindings_sys as sys;

use crate::value::AttrsetValue;
use crate::{Context, Result, Utf8CStr, Value, ValueKind};

/// TODO: docs.
pub trait Attrset: Sized {
    /// Returns the index of the attribute with the given key, or `None` if no
    /// such key exists.
    ///
    /// If an index is returned, it is guaranteed to be less than `self.len()`.
    fn get_idx_of_key(&self, key: &str) -> Option<usize>;

    /// Returns the key of the attribute at the given index.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds (i.e. greater than or equal to
    /// `self.len()`).
    fn get_key(&self, idx: usize) -> &str;

    /// Same as [`get_key_by_idx`](Attrset::get_key_by_idx), but returns the
    /// key as a `&CStr`.
    fn get_key_as_cstr(&self, idx: usize) -> &CStr;

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
    unsafe fn write_value(
        &self,
        idx: usize,
        dest: NonNull<sys::Value>,
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
    fn call(self, value: &'a impl AsRef<Utf8CStr>) -> T;
}

/// TODO: docs.
pub trait FnOnceValue<'a, T: 'a> {
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
    fn get_idx_of_key(&self, _key: &str) -> Option<usize> {
        unimplemented!()
    }

    #[inline]
    fn get_key(&self, _idx: usize) -> &str {
        unimplemented!()
    }

    #[inline]
    fn get_key_as_cstr(&self, _idx: usize) -> &CStr {
        unimplemented!()
    }

    #[inline]
    fn get_value_kind(&self, _idx: usize) -> ValueKind {
        unimplemented!()
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
        _dest: NonNull<sys::Value>,
        _ctx: &mut Context,
    ) -> Result<()> {
        debug_assert!(idx < self.len());
        todo!();
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
    fn get_key_as_cstr(&self, idx: usize) -> &CStr {
        (*self).get_key_as_cstr(idx)
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
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { (*self).write_value(idx, dest, ctx) }
    }
}
