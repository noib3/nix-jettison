use core::fmt;

/// TODO: docs.
pub type Result<T> = core::result::Result<T, Error>;

/// TODO: docs.
#[derive(Debug)]
pub enum Error {
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

impl fmt::Display for Error {
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

impl core::error::Error for Error {}
