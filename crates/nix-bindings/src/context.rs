use core::ffi::CStr;
use core::ptr::NonNull;

use {nix_bindings_cpp as cpp, nix_bindings_sys as sys};

use crate::{Error, ErrorKind, PrimOp, Result, ToError};

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

pub(crate) struct BindingsBuilder<'ctx> {
    inner: NonNull<sys::BindingsBuilder>,
    context: &'ctx mut Context,
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

impl Context<EvalState> {
    /// TODO: docs.
    #[inline]
    pub(crate) fn make_bindings_builder(
        &mut self,
        capacity: usize,
    ) -> Result<BindingsBuilder<'_>> {
        unsafe {
            let builder_ptr = cpp::make_bindings_builder(
                self.state.inner.as_ptr(),
                capacity,
            );
            match NonNull::new(builder_ptr) {
                Some(builder_ptr) => {
                    Ok(BindingsBuilder { inner: builder_ptr, context: self })
                },
                None => Err(self.make_error((
                    ErrorKind::Overflow,
                    c"failed to create BindingsBuilder",
                ))),
            }
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

impl<'ctx> BindingsBuilder<'ctx> {
    #[inline]
    pub(crate) fn insert(
        &mut self,
        key: &CStr,
        write_value: impl FnOnce(NonNull<sys::Value>, &mut Context) -> Result<()>,
    ) -> Result<()> {
        unsafe {
            let dest_raw = cpp::alloc_value(self.context.state.inner.as_ptr());

            let dest_ptr = NonNull::new(dest_raw).ok_or_else(|| {
                self.context.make_error((
                    ErrorKind::Overflow,
                    c"failed to allocate Value for BindingsBuilder insert",
                ))
            })?;

            write_value(dest_ptr, self.context)?;

            cpp::bindings_builder_insert(
                self.inner.as_ptr(),
                key.as_ptr(),
                dest_ptr.as_ptr(),
            );

            Ok(())
        }
    }

    #[inline]
    pub(crate) fn build(self, dest: NonNull<sys::Value>) -> Result<()> {
        unsafe {
            cpp::make_attrs(dest.as_ptr(), self.inner.as_ptr());
            Ok(())
        }
    }
}
