use core::ptr::NonNull;

use nix_bindings_sys as sys;

use crate::{PrimOp, Result};

/// TODO: docs.
pub struct Context<State = EvalState> {
    inner: NonNull<sys::c_context>,
    state: State,
}

/// TODO: docs.
pub struct Entrypoint {}

/// TODO: docs.
pub struct EvalState {
    inner: NonNull<sys::EvalState>,
}

impl Context<Entrypoint> {
    /// Adds the given primop to the `builtins` attribute set.
    #[track_caller]
    #[inline]
    pub fn register_primop<P: PrimOp>(&mut self, primop: P) {
        let try_block = || unsafe {
            let primop_ptr = self.with_inner(|ctx| primop.alloc(ctx))?;
            self.with_inner_raw(|ctx| sys::register_primop(ctx, primop_ptr))?;
            self.with_inner_raw(|ctx| sys::gc_decref(ctx, primop_ptr.cast()))?;
            Result::Ok(())
        };

        if let Err(err) = try_block() {
            panic!("couldn't register primop '{:?}': {}", P::NAME, err);
        }
    }

    /// TODO: docs.
    #[inline]
    fn with_inner<T>(
        &mut self,
        _f: impl FnOnce(NonNull<sys::c_context>) -> T,
    ) -> Result<T> {
        todo!();
    }

    /// Same as [`with_inner`](Self::with_inner), but provides the callback
    /// with a raw pointer instead of a `NonNull`.
    #[inline]
    fn with_inner_raw<T>(
        &mut self,
        _f: impl FnOnce(*mut sys::c_context) -> T,
    ) -> Result<T> {
        todo!();
    }
}

impl<State> Context<State> {
    #[inline]
    pub(crate) fn new(inner: NonNull<sys::c_context>, state: State) -> Self {
        Self { inner, state }
    }
}

impl EvalState {
    #[inline]
    pub(crate) fn new(inner: NonNull<sys::EvalState>) -> Self {
        Self { inner }
    }
}
