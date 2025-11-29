//! TODO: docs.

#![allow(clippy::undocumented_unsafe_blocks)]
#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(nightly, feature(generic_const_exprs))]

extern crate alloc;

pub mod attrset;
pub mod builtins;
pub mod context;
mod entry;
pub mod error;
pub mod function;
pub mod list;
mod namespace;
pub mod primop;
pub mod thunk;
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
    pub use crate::function::*;
    pub use crate::list::*;
    pub use crate::primop::*;
    pub use crate::thunk::*;
    pub use crate::value::*;
}
