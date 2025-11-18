use core::fmt;

/// TODO: docs.
pub type Result<T> = core::result::Result<T, Error>;

/// TODO: docs.
#[derive(Debug)]
pub enum Error {}

impl fmt::Display for Error {
    #[inline]
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

impl core::error::Error for Error {}
