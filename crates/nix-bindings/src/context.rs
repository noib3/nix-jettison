//! TODO: docs.

use core::ffi::CStr;
use core::ptr::NonNull;

use {nix_bindings_cpp as cpp, nix_bindings_sys as sys};

use crate::error::{Error, ErrorKind, Result, ToError};
use crate::namespace::Namespace;
use crate::primop::PrimOp;
use crate::value::{TryFromValue, ValueKind};

/// TODO: docs.
pub struct Context<State = EvalState> {
    inner: ContextInner,
    state: State,
}

/// TODO: docs.
pub struct Entrypoint {}

/// TODO: docs.
pub struct EvalState {
    inner: NonNull<sys::EvalState>,
}

pub(crate) struct AttrsetBuilder<'ctx> {
    inner: NonNull<sys::BindingsBuilder>,
    context: &'ctx mut Context,
}

pub(crate) struct ListBuilder<'ctx> {
    inner: NonNull<sys::ListBuilder>,
    context: &'ctx mut Context,
    index: usize,
}

pub(crate) struct ContextInner {
    ptr: NonNull<sys::c_context>,
}

impl Context<Entrypoint> {
    /// Adds the given primop to the `builtins` attribute set.
    #[track_caller]
    #[inline]
    pub fn register_primop<P: PrimOp>(&mut self) {
        let res = self.inner.with_primop_ptr::<P, _>(
            P::NAME,
            |inner, primop_ptr| {
                inner.with_raw(|raw_ctx| unsafe {
                    sys::register_primop(raw_ctx, primop_ptr.as_ptr())
                })
            },
        );

        if let Err(err) = res {
            panic!("couldn't register primop '{:?}': {err}", P::NAME);
        }
    }
}

impl Context<EvalState> {
    /// Forces the evaluation of the given value.
    ///
    /// The value's kind is guaranteed to not be [`ValueKind::Thunk`] after
    /// a successful call to this method.
    #[inline]
    pub(crate) fn force(&mut self, value: NonNull<sys::Value>) -> Result<()> {
        unsafe {
            cpp::force_value(self.state.inner.as_ptr(), value.as_ptr());
        }
        Ok(())
    }

    /// Returns the kind of the given value.
    #[inline]
    pub(crate) fn get_kind(
        &mut self,
        value: NonNull<sys::Value>,
    ) -> Result<ValueKind> {
        Ok(
            match self.inner.with_raw(|ctx| unsafe {
                sys::get_type(ctx, value.as_ptr())
            })? {
                sys::ValueType_NIX_TYPE_ATTRS => ValueKind::Attrset,
                sys::ValueType_NIX_TYPE_BOOL => ValueKind::Bool,
                sys::ValueType_NIX_TYPE_EXTERNAL => ValueKind::External,
                sys::ValueType_NIX_TYPE_FLOAT => ValueKind::Float,
                sys::ValueType_NIX_TYPE_FUNCTION => ValueKind::Function,
                sys::ValueType_NIX_TYPE_INT => ValueKind::Int,
                sys::ValueType_NIX_TYPE_LIST => ValueKind::List,
                sys::ValueType_NIX_TYPE_NULL => ValueKind::Null,
                sys::ValueType_NIX_TYPE_PATH => ValueKind::Path,
                sys::ValueType_NIX_TYPE_STRING => ValueKind::String,
                sys::ValueType_NIX_TYPE_THUNK => ValueKind::Thunk,
                other => unreachable!("invalid ValueType: {other}"),
            },
        )
    }

    /// Creates a new [`AttrsetBuilder`] with the given capacity.
    #[inline]
    pub(crate) fn make_attrset_builder(
        &mut self,
        capacity: usize,
    ) -> Result<AttrsetBuilder<'_>> {
        unsafe {
            let builder_ptr = cpp::make_bindings_builder(
                self.state.inner.as_ptr(),
                capacity,
            );
            match NonNull::new(builder_ptr) {
                Some(builder_ptr) => {
                    Ok(AttrsetBuilder { inner: builder_ptr, context: self })
                },
                None => Err(self.make_error((
                    ErrorKind::Overflow,
                    c"failed to create AttrsetBuilder",
                ))),
            }
        }
    }

    /// Creates a new [`ListBuilder`] with the given capacity.
    #[inline]
    pub(crate) fn make_list_builder(
        &mut self,
        capacity: usize,
    ) -> Result<ListBuilder<'_>> {
        unsafe {
            let builder_ptr =
                cpp::make_list_builder(self.state.inner.as_ptr(), capacity);
            match NonNull::new(builder_ptr) {
                Some(builder_ptr) => Ok(ListBuilder {
                    inner: builder_ptr,
                    context: self,
                    index: 0,
                }),
                None => Err(self.make_error((
                    ErrorKind::Overflow,
                    c"failed to create ListBuilder",
                ))),
            }
        }
    }

    /// Initializes the destination value with the given primop.
    #[inline]
    pub(crate) fn write_primop<P: PrimOp>(
        &mut self,
        namespace: impl Namespace,
        dest: NonNull<sys::Value>,
    ) -> Result<()> {
        self.inner
            .with_primop_ptr::<P, _>(namespace, |ctx, primop_ptr| {
                ctx.with_raw(|raw_ctx| unsafe {
                    sys::init_primop(
                        raw_ctx,
                        dest.as_ptr(),
                        primop_ptr.as_ptr(),
                    );
                })
            })
            .flatten()
    }

    /// Gets the argument at the given offset and tries to convert it to
    /// the desired type.
    ///
    /// Returns an error if the pointer at the given offset is NULL or if the
    /// conversion fails.
    ///
    /// This is only meant to be used in the code generated by the
    /// [`Args`](crate::derive::Args) derive macro, and is not part of
    /// `Context`'s public API.
    #[doc(hidden)]
    #[inline]
    pub unsafe fn get_arg<T: TryFromValue>(
        &mut self,
        args: NonNull<*mut sys::Value>,
        offset: u8,
    ) -> Result<T> {
        let arg_raw = unsafe { *args.as_ptr().offset(offset.into()) };
        let arg_ptr = NonNull::new(arg_raw).ok_or_else(|| {
            self.make_error((ErrorKind::Overflow, c"argument is NULL"))
        })?;
        unsafe { T::try_from_value(arg_ptr, self) }
    }
}

impl<State> Context<State> {
    /// TODO: docs.
    #[inline]
    pub(crate) fn make_error(&mut self, err: impl ToError) -> Error {
        self.inner.make_error(err)
    }

    #[inline]
    pub(crate) fn new(ctx_ptr: NonNull<sys::c_context>, state: State) -> Self {
        Self { inner: ContextInner::new(ctx_ptr), state }
    }

    /// TODO: docs.
    #[inline]
    pub(crate) fn with_raw<T>(
        &mut self,
        fun: impl FnOnce(*mut sys::c_context) -> T,
    ) -> Result<T> {
        self.inner.with_raw(fun)
    }

    /// TODO: docs.
    #[inline]
    pub(crate) fn with_raw_and_state<T>(
        &mut self,
        fun: impl FnOnce(*mut sys::c_context, &mut State) -> T,
    ) -> Result<T> {
        self.inner.with_raw(|raw_ctx| fun(raw_ctx, &mut self.state))
    }
}

impl EvalState {
    #[inline]
    pub(crate) fn as_ptr(&mut self) -> *mut sys::EvalState {
        self.inner.as_ptr()
    }

    #[inline]
    pub(crate) fn new(inner: NonNull<sys::EvalState>) -> Self {
        Self { inner }
    }
}

impl<'ctx> AttrsetBuilder<'ctx> {
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
                    c"failed to allocate Value for AttrsetBuilder insert",
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

impl<'ctx> ListBuilder<'ctx> {
    #[inline]
    pub(crate) fn insert(
        &mut self,
        write_value: impl FnOnce(NonNull<sys::Value>, &mut Context) -> Result<()>,
    ) -> Result<()> {
        unsafe {
            let dest_raw = cpp::alloc_value(self.context.state.inner.as_ptr());

            let dest_ptr = NonNull::new(dest_raw).ok_or_else(|| {
                self.context.make_error((
                    ErrorKind::Overflow,
                    c"failed to allocate Value for ListBuilder insert",
                ))
            })?;

            write_value(dest_ptr, self.context)?;

            cpp::list_builder_insert(
                self.inner.as_ptr(),
                self.index,
                dest_ptr.as_ptr(),
            );
            self.index += 1;

            Ok(())
        }
    }

    #[inline]
    pub(crate) fn build(self, dest: NonNull<sys::Value>) -> Result<()> {
        unsafe {
            cpp::make_list(dest.as_ptr(), self.inner.as_ptr());
            Ok(())
        }
    }
}

impl ContextInner {
    /// TODO: docs.
    #[inline]
    pub(crate) fn make_error(&mut self, err: impl ToError) -> Error {
        unsafe {
            let kind = err.kind();
            let message = err.format_to_c_str();
            sys::set_err_msg(self.ptr.as_ptr(), kind.code(), message.as_ptr());
            #[expect(deprecated)]
            Error::new(kind, self)
        }
    }

    #[inline]
    pub(crate) fn new(inner: NonNull<sys::c_context>) -> Self {
        Self { ptr: inner }
    }

    /// TODO: docs.
    #[inline]
    pub(crate) fn with_ptr<T>(
        &mut self,
        fun: impl FnOnce(NonNull<sys::c_context>) -> T,
    ) -> Result<T> {
        let ret = fun(self.ptr);
        self.check_err().map(|()| ret)
    }

    /// Same as [`with_raw`](Self::with_raw), but provides the callback with a
    /// raw pointer instead of a `NonNull`.
    #[inline]
    pub(crate) fn with_raw<T>(
        &mut self,
        fun: impl FnOnce(*mut sys::c_context) -> T,
    ) -> Result<T> {
        let ret = fun(self.ptr.as_ptr());
        self.check_err().map(|()| ret)
    }

    #[inline]
    fn check_err(&mut self) -> Result<()> {
        let kind = match unsafe { sys::err_code(self.ptr.as_ptr()) } {
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

    #[inline]
    fn with_primop_ptr<P: PrimOp, T>(
        &mut self,
        namespace: impl Namespace,
        fun: impl FnOnce(&mut Self, NonNull<sys::PrimOp>) -> T,
    ) -> Result<T> {
        // TODO: alloc() is implemented by leaking, so calling this repeatedly
        // will cause memory leaks. Fix this.
        let primop_raw = self
            .with_ptr(|ctx| unsafe { P::alloc(namespace.display(), ctx) })?;

        let primop_ptr = NonNull::new(primop_raw).ok_or_else(|| {
            self.make_error((
                ErrorKind::Overflow,
                c"failed to allocate PrimOp for {primop:?}",
            ))
        })?;

        let ret = fun(self, primop_ptr);

        self.with_raw(|ctx| unsafe {
            sys::gc_decref(ctx, primop_ptr.as_ptr().cast())
        })?;

        Result::Ok(ret)
    }
}
