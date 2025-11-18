use core::ptr::NonNull;

use nix_bindings_sys as sys;

use crate::{Error, PrimOp, Result};

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
}

impl<State> Context<State> {
    #[inline]
    pub(crate) fn new(inner: NonNull<sys::c_context>, state: State) -> Self {
        Self { inner, state }
    }

    /// TODO: docs.
    #[inline]
    pub(crate) fn with_inner<T>(
        &mut self,
        fun: impl FnOnce(NonNull<sys::c_context>) -> T,
    ) -> Result<T> {
        let ret = fun(self.inner);
        self.check_inner().map(|()| ret)
    }

    /// Same as [`with_inner`](Self::with_inner), but provides the callback
    /// with a raw pointer instead of a `NonNull`.
    #[inline]
    pub(crate) fn with_inner_raw<T>(
        &mut self,
        fun: impl FnOnce(*mut sys::c_context) -> T,
    ) -> Result<T> {
        let ret = fun(self.inner.as_ptr());
        self.check_inner().map(|()| ret)
    }

    #[inline]
    fn check_inner(&mut self) -> Result<()> {
        match unsafe { sys::err_code(self.inner.as_ptr()) } {
            sys::err_NIX_OK => Ok(()),
            sys::err_NIX_ERR_UNKNOWN => Err(Error::Unknown),
            sys::err_NIX_ERR_OVERFLOW => Err(Error::Overflow),
            sys::err_NIX_ERR_KEY => Err(Error::Key),
            sys::err_NIX_ERR_NIX_ERROR => Err(Error::Nix),
            other => unreachable!("invalid error code: {other}"),
        }
    }
}

impl EvalState {
    #[inline]
    pub(crate) fn new(inner: NonNull<sys::EvalState>) -> Self {
        Self { inner }
    }
}
