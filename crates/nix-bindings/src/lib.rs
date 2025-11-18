//! TODO: docs.

mod context;
mod error;
mod primop;
mod value;

pub use context::{Context, Entrypoint, EvalState};
pub use error::{Error, Result};
pub use primop::{PrimOp, PrimOpFun};
pub use value::Value;
