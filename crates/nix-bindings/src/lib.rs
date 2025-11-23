//! TODO: docs.

#![allow(clippy::undocumented_unsafe_blocks)]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod attrset;
pub mod context;
mod entry;
pub mod error;
pub mod list;
mod namespace;
pub mod primop;
mod utf8_cstr;
pub mod value;

#[doc(hidden)]
pub use entry::entry;
pub use nix_bindings_macros::{Args, PrimOp, TryFromValue, entry};
#[doc(hidden)]
pub use nix_bindings_sys as sys;
pub use utf8_cstr::Utf8CStr;

pub mod prelude {
    //! TODO: docs.

    pub use crate::Utf8CStr;
    pub use crate::attrset::*;
    pub use crate::context::*;
    pub use crate::error::*;
    pub use crate::list::*;
    pub use crate::primop::*;
    pub use crate::value::*;
}
