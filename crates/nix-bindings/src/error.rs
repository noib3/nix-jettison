//! TODO: docs.

use core::ffi::CStr;
use core::fmt;
use std::borrow::Cow;
use std::ffi::CString;

use nix_bindings_sys as sys;

use crate::prelude::{Context, ValueKind};

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

impl Error {
    #[deprecated = "use Context::make_error instead"]
    pub(crate) fn new<S>(kind: ErrorKind, _: &mut Context<S>) -> Self {
        Self { kind }
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

impl ToError for std::ffi::NulError {
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
