//! TODO: docs.

#![allow(clippy::undocumented_unsafe_blocks)]

mod context;
mod entry;
mod error;
mod primop;
mod value;

pub use context::{Context, Entrypoint, EvalState};
#[doc(hidden)]
pub use entry::entry;
pub use error::{Error, ErrorKind, ToError, Result};
pub use primop::{PrimOp, PrimOpFun};
pub use value::Value;
