//! TODO: docs.

#![allow(clippy::undocumented_unsafe_blocks)]

mod attrset;
mod context;
mod entry;
mod error;
mod primop;
mod utf8_cstr;
mod value;

pub use attrset::{Attrset, LiteralAttrset};
pub use context::{Context, Entrypoint, EvalState};
#[doc(hidden)]
pub use entry::entry;
pub use error::{Error, ErrorKind, Result, ToError};
pub use primop::{PrimOp, PrimOpFun};
pub use utf8_cstr::Utf8CStr;
pub use value::{TryIntoValue, Value, ValueKind};
