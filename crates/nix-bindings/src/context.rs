use core::ptr::NonNull;

use crate::{PrimOp, Result};

/// TODO: docs.
pub struct Context<State = EvalState> {
    inner: NonNull<nix_bindings_sys::c_context>,
    state: State,
}

/// TODO: docs.
pub struct Entrypoint {}

/// TODO: docs.
pub struct EvalState {
    inner: NonNull<nix_bindings_sys::EvalState>,
}

impl Context<Entrypoint> {
    /// Adds the given primop to the `builtins` attribute set.
    pub fn register_primop(&mut self, _primop: impl PrimOp) -> Result<()> {
        todo!();
    }
}
