use core::ffi::CStr;
use core::ptr::NonNull;

use nix_bindings_sys as sys;

use crate::value::AttrsetValue;
use crate::{Context, Result, Value, ValueKind};

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
