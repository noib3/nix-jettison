//! TODO: docs.

use alloc::borrow::Cow;
use alloc::ffi::CString;
use alloc::string::ToString;
use core::ffi::CStr;
use core::fmt;
use core::marker::PhantomData;

use nix_bindings_sys as sys;

use crate::context::ContextInner;
use crate::value::ValueKind;

/// TODO: docs.
pub type Result<T> = core::result::Result<T, Error>;

/// TODO: docs.
pub trait ToError {
    /// TODO: docs.
    fn kind(&self) -> ErrorKind;

    /// TODO: docs.
    fn format_to_c_str(&self) -> Cow<'_, CStr>;
}

/// TODO: docs.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
}

/// TODO: docs.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// An unknown error occurred.
    ///
    /// This error code is returned when an unknown error occurred during the
    /// function execution.
    Unknown,

    /// An overflow error occurred.
    ///
    /// This error code is returned when an overflow error occurred during the
    /// function execution.
    Overflow,

    /// A key/index access error occurred in C API functions.
    ///
    /// This error code is returned when accessing a key, index, or identifier
    /// that does not exist in C API functions. Common scenarios include:
    ///
    /// - setting keys that don't exist;
    /// - list indices that are out of bounds;
    /// - attribute names that don't exist;
    /// - attribute indices that are out of bounds;
    ///
    /// This error typically indicates incorrect usage or assumptions about
    /// data structure contents, rather than internal Nix evaluation errors.
    Key,

    /// A generic Nix error occurred.
    ///
    /// This error code is returned when a generic Nix error occurred during
    /// the function execution.
    Nix,
}

/// The type of error that can occur when trying to convert a generic value
/// to a specific type.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct TypeMismatchError {
    /// The expected value kind.
    pub expected: ValueKind,

    /// The found value kind.
    pub found: ValueKind,
}

/// The type of error that can occur when trying to convert an `i64` into a
/// different integer type.
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct TryFromI64Error<Int> {
    n: i64,
    int: PhantomData<Int>,
}

impl Error {
    #[deprecated = "use Context::make_error instead"]
    #[inline]
    pub(crate) fn new(kind: ErrorKind, _: &mut ContextInner) -> Self {
        Self { kind }
    }
}

impl<Int> TryFromI64Error<Int> {
    #[inline]
    pub(crate) fn new(n: i64) -> Self {
        Self { n, int: PhantomData }
    }
}

impl ErrorKind {
    #[inline]
    pub(crate) fn code(self) -> sys::err {
        match self {
            Self::Unknown => sys::err_NIX_ERR_UNKNOWN,
            Self::Overflow => sys::err_NIX_ERR_OVERFLOW,
            Self::Key => sys::err_NIX_ERR_KEY,
            Self::Nix => sys::err_NIX_ERR_NIX_ERROR,
        }
    }
}

impl fmt::Display for Error {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl core::error::Error for Error {}

impl fmt::Display for ErrorKind {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match *self {
            Self::Unknown => "an unknown error occurred",
            Self::Overflow => "an overflow error occurred",
            Self::Key => "a key/index access error occurred",
            Self::Nix => "a generic Nix error occurred",
        })
    }
}

impl fmt::Display for TypeMismatchError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "type mismatch: expected {:?}, found {:?}",
            self.expected, self.found
        )
    }
}

impl core::error::Error for TypeMismatchError {}

impl<Int> fmt::Debug for TryFromI64Error<Int> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TryFromI64Error")
            .field("n", &self.n)
            .field("int", &core::any::type_name::<Int>())
            .finish()
    }
}

impl<Int> fmt::Display for TryFromI64Error<Int> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "integer conversion failed: cannot convert {}i64 into target \
             type {}",
            self.n,
            core::any::type_name::<Int>()
        )
    }
}

impl<Int> core::error::Error for TryFromI64Error<Int> {}

impl ToError for core::convert::Infallible {
    #[inline]
    fn format_to_c_str(&self) -> Cow<'_, CStr> {
        unreachable!()
    }

    #[inline]
    fn kind(&self) -> ErrorKind {
        unreachable!()
    }
}

impl ToError for TypeMismatchError {
    #[inline]
    fn format_to_c_str(&self) -> Cow<'_, CStr> {
        // SAFETY: the Display impl doesn't contain any NUL bytes.
        unsafe { CString::from_vec_unchecked(self.to_string().into()).into() }
    }

    #[inline]
    fn kind(&self) -> ErrorKind {
        ErrorKind::Nix
    }
}

impl ToError for alloc::ffi::NulError {
    #[inline]
    fn format_to_c_str(&self) -> Cow<'_, CStr> {
        // SAFETY: NulError's Display impl doesn't contain any NUL bytes.
        unsafe { CString::from_vec_unchecked(self.to_string().into()) }.into()
    }

    #[inline]
    fn kind(&self) -> ErrorKind {
        ErrorKind::Nix
    }
}

impl ToError for alloc::ffi::IntoStringError {
    #[inline]
    fn format_to_c_str(&self) -> Cow<'_, CStr> {
        c"C string contained non-utf8 bytes".into()
    }

    #[inline]
    fn kind(&self) -> ErrorKind {
        ErrorKind::Nix
    }
}

impl ToError for core::str::Utf8Error {
    #[inline]
    fn format_to_c_str(&self) -> Cow<'_, CStr> {
        // SAFETY: NulError's Display impl doesn't contain any NUL bytes.
        unsafe { CString::from_vec_unchecked(self.to_string().into()) }.into()
    }

    #[inline]
    fn kind(&self) -> ErrorKind {
        ErrorKind::Nix
    }
}

impl<Int> ToError for TryFromI64Error<Int> {
    #[inline]
    fn format_to_c_str(&self) -> Cow<'_, CStr> {
        // SAFETY: the Display impl doesn't contain any NUL bytes.
        unsafe { CString::from_vec_unchecked(self.to_string().into()) }.into()
    }

    #[inline]
    fn kind(&self) -> ErrorKind {
        ErrorKind::Nix
    }
}

impl ToError for (ErrorKind, &CStr) {
    #[inline]
    fn format_to_c_str(&self) -> Cow<'_, CStr> {
        Cow::Borrowed(self.1)
    }

    #[inline]
    fn kind(&self) -> ErrorKind {
        self.0
    }
}
