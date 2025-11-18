use core::ffi::CStr;

use crate::{Context, Result, Value};

const MAX_ARITY: u8 = 8;

/// TODO: docs.
pub trait PrimOp: PrimOpFun + 'static {
    /// TODO: docs.
    const NAME: &'static CStr;

    /// TODO: docs.
    const DOCS: &'static CStr;
}

/// TODO: docs.
pub trait PrimOpFun {
    /// TODO: docs.
    type Args: Args;

    /// TODO: docs.
    fn call(&self, args: Self::Args, ctx: &mut Context) -> Result<impl Value>;
}

/// TODO: docs.
pub trait Args {
    #[doc(hidden)]
    const MAX_ARITY_CHECK: () = assert!(Self::ARITY <= MAX_ARITY);

    /// TODO: docs.
    const ARITY: u8;
}
