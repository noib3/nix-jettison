use core::ptr::NonNull;

use nix_bindings_sys as sys;

use crate::{Error, ErrorKind, ToError, PrimOp, Result};

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
    /// TODO: docs.
    #[inline]
    pub(crate) fn make_error(&mut self, err: impl ToError) -> Error {
        unsafe {
            let kind = err.kind();
            let message = err.format_to_c_str();
            sys::set_err_msg(
                self.inner.as_ptr(),
                kind.code(),
                message.as_ptr(),
            );
            #[expect(deprecated)]
            Error::new(kind, self)
        }
    }

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
        let kind = match unsafe { sys::err_code(self.inner.as_ptr()) } {
            sys::err_NIX_OK => return Ok(()),
            sys::err_NIX_ERR_UNKNOWN => ErrorKind::Unknown,
            sys::err_NIX_ERR_OVERFLOW => ErrorKind::Overflow,
            sys::err_NIX_ERR_KEY => ErrorKind::Key,
            sys::err_NIX_ERR_NIX_ERROR => ErrorKind::Nix,
            other => unreachable!("invalid error code: {other}"),
        };
        #[expect(deprecated)]
        Err(Error::new(kind, self))
    }
}

impl EvalState {
    #[inline]
    pub(crate) fn new(inner: NonNull<sys::EvalState>) -> Self {
        Self { inner }
    }
}
