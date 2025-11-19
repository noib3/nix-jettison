use core::ffi::CStr;
use core::str::Utf8Error;

/// A wrapper around a [`CStr`] that is guaranteed to contain valid UTF-8.
#[repr(transparent)]
pub struct Utf8CStr(CStr);

impl Utf8CStr {
    /// Creates a new [`Utf8CStr`] from the given [`CStr`], without checking
    /// whether it contains valid UTF-8.
    #[inline]
    pub fn new(cstr: &CStr) -> Result<&Self, Utf8Error> {
        cstr.to_str().map(|_| {
            // SAFETY: `cstr` contains valid UTF-8.
            unsafe { Self::new_unchecked(cstr) }
        })
    }

    /// Creates a new [`Utf8CStr`] from the given [`CStr`], without checking
    /// whether it contains valid UTF-8.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the given `CStr` contains valid UTF-8.
    #[inline]
    pub unsafe fn new_unchecked(cstr: &CStr) -> &Self {
        debug_assert!(
            cstr.to_str().is_ok(),
            "Utf8CStr::new_unchecked called with invalid UTF-8 CStr"
        );
        // SAFETY: the caller guarantees that `cstr` contains valid UTF-8, and
        // `Self` is #[repr(transparent)] over `CStr`.
        unsafe { &*(cstr as *const CStr as *const Self) }
    }
}

impl AsRef<Self> for Utf8CStr {
    #[inline]
    fn as_ref(&self) -> &Self {
        self
    }
}
