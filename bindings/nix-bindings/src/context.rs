//! TODO: docs.

use alloc::borrow::ToOwned;
use core::ffi::CStr;
use core::marker::PhantomData;
use core::ptr::{self, NonNull};
use core::slice;

use {nix_bindings_cpp as cpp, nix_bindings_sys as sys};

use crate::attrset::NixAttrset;
use crate::builtins::Builtins;
use crate::error::{Error, ErrorKind, Result};
use crate::namespace::Namespace;
use crate::primop::PrimOp;
use crate::value::{NixValue, TryFromValue};

/// TODO: docs.
pub struct Context<'state, State = EvalState<'state>> {
    inner: ContextInner,
    state: State,
    _lifetime: PhantomData<&'state ()>,
}

/// TODO: docs.
pub struct Entrypoint {}

/// TODO: docs.
pub struct EvalState<'a> {
    inner: NonNull<sys::EvalState>,
    _lifetime: PhantomData<&'a sys::EvalState>,
}

pub(crate) struct AttrsetBuilder<'ctx, 'eval> {
    inner: NonNull<sys::BindingsBuilder>,
    context: &'ctx mut Context<'eval>,
}

pub(crate) struct ListBuilder<'ctx, 'eval> {
    inner: NonNull<sys::ListBuilder>,
    context: &'ctx mut Context<'eval>,
    index: usize,
}

pub(crate) struct ContextInner {
    ptr: NonNull<sys::c_context>,
}

impl Context<'_, Entrypoint> {
    /// Adds the given primop to the `builtins` attribute set.
    #[track_caller]
    #[inline]
    pub fn register_primop<P: PrimOp>(&mut self) {
        let res =
            self.inner.with_primop_ptr::<P, _>(P::NAME, |inner, primop_ptr| {
                inner.with_raw(|raw_ctx| unsafe {
                    sys::register_primop(raw_ctx, primop_ptr.as_ptr())
                })
            });

        if let Err(err) = res {
            panic!("couldn't register primop '{:?}': {err}", P::NAME);
        }
    }
}

impl<'eval> Context<'eval> {
    /// Returns the global `builtins` attribute set.
    ///
    /// This provides access to all built-in functions like `fetchGit`,
    /// `fetchurl`, `toString`, etc.
    #[inline]
    pub fn builtins(&mut self) -> Builtins<'eval> {
        let builtins_raw = unsafe { cpp::get_builtins(self.state.as_ptr()) };

        let Some(builtins_ptr) = NonNull::new(builtins_raw) else {
            panic!("failed to get builtins attrset: got null pointer");
        };

        // SAFETY: the value returned by `get_builtins` is initialized.
        let builtins_value = unsafe { NixValue::new(builtins_ptr) };

        match NixAttrset::try_from_value(builtins_value, self) {
            Ok(attrset) => Builtins::new(attrset),
            Err(err) => unreachable!("builtins is not an attrset: {err}"),
        }
    }

    /// TODO: docs.
    #[inline]
    pub fn eval<T>(&mut self, expr: &CStr) -> Result<T>
    where
        T: TryFromValue<NixValue<'static>>,
    {
        let dest = self.alloc_value()?;

        self.with_raw_and_state(|raw_ctx, state| unsafe {
            cpp::expr_eval_from_string(
                raw_ctx,
                state.as_ptr(),
                expr.as_ptr(),
                c".".as_ptr(),
                dest.as_ptr(),
            );
        })?;

        // SAFETY: `expr_eval_from_string` has initialized the value.
        let value = unsafe { NixValue::new(dest) };

        T::try_from_value(value, self)
    }

    /// Allocates a new, uninitialized value, returning a pointer to it.
    ///
    /// The caller is responsible for freeing the value by calling
    /// [`sys::value_decref`] once it is no longer needed.
    #[inline]
    pub(crate) fn alloc_value(&mut self) -> Result<NonNull<sys::Value>> {
        let raw_ptr = unsafe { cpp::alloc_value(self.state.inner.as_ptr()) };

        NonNull::new(raw_ptr).ok_or_else(|| {
            Error::new(ErrorKind::Overflow, c"failed to allocate Value")
        })
    }

    /// Creates a new [`AttrsetBuilder`] with the given capacity.
    #[inline]
    pub(crate) fn make_attrset_builder(
        &mut self,
        capacity: usize,
    ) -> Result<AttrsetBuilder<'_, 'eval>> {
        unsafe {
            let builder_ptr =
                cpp::make_bindings_builder(self.state.inner.as_ptr(), capacity);
            match NonNull::new(builder_ptr) {
                Some(builder_ptr) => {
                    Ok(AttrsetBuilder { inner: builder_ptr, context: self })
                },
                None => Err(Error::new(
                    ErrorKind::Overflow,
                    c"failed to create AttrsetBuilder",
                )),
            }
        }
    }

    /// Creates a new [`ListBuilder`] with the given capacity.
    #[inline]
    pub(crate) fn make_list_builder(
        &mut self,
        capacity: usize,
    ) -> Result<ListBuilder<'_, 'eval>> {
        unsafe {
            let builder_ptr =
                cpp::make_list_builder(self.state.inner.as_ptr(), capacity);
            match NonNull::new(builder_ptr) {
                Some(builder_ptr) => Ok(ListBuilder {
                    inner: builder_ptr,
                    context: self,
                    index: 0,
                }),
                None => Err(Error::new(
                    ErrorKind::Overflow,
                    c"failed to create ListBuilder",
                )),
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
}

impl<State> Context<'_, State> {
    #[inline]
    pub(crate) fn inner_mut(&mut self) -> &mut ContextInner {
        &mut self.inner
    }

    #[inline]
    pub(crate) fn new(ctx_ptr: NonNull<sys::c_context>, state: State) -> Self {
        Self {
            inner: ContextInner::new(ctx_ptr),
            state,
            _lifetime: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn state_mut(&mut self) -> &mut State {
        &mut self.state
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

impl EvalState<'_> {
    #[inline]
    pub(crate) fn as_ptr(&mut self) -> *mut sys::EvalState {
        self.inner.as_ptr()
    }

    #[inline]
    pub(crate) fn new(inner: NonNull<sys::EvalState>) -> Self {
        Self { inner, _lifetime: PhantomData }
    }
}

impl<'eval> AttrsetBuilder<'_, 'eval> {
    #[inline]
    pub(crate) fn build(self, dest: NonNull<sys::Value>) -> Result<()> {
        unsafe {
            cpp::make_attrs(dest.as_ptr(), self.inner.as_ptr());
            Ok(())
        }
    }

    #[inline]
    pub(crate) fn ctx(&mut self) -> &mut Context<'eval> {
        self.context
    }

    #[inline]
    pub(crate) fn insert(
        &mut self,
        key: &CStr,
        write_value: impl FnOnce(NonNull<sys::Value>, &mut Context) -> Result<()>,
    ) -> Result<()> {
        unsafe {
            let dest_ptr = self.context.alloc_value()?;

            write_value(dest_ptr, self.context)?;

            cpp::bindings_builder_insert(
                self.inner.as_ptr(),
                key.as_ptr(),
                dest_ptr.as_ptr(),
            );

            Ok(())
        }
    }
}

impl ListBuilder<'_, '_> {
    #[inline]
    pub(crate) fn insert(
        &mut self,
        write_value: impl FnOnce(NonNull<sys::Value>, &mut Context) -> Result<()>,
    ) -> Result<()> {
        unsafe {
            let dest_ptr = self.context.alloc_value()?;

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
    #[inline]
    pub(crate) fn as_raw(&mut self) -> *mut sys::c_context {
        self.ptr.as_ptr()
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
        let ret = fun(self.as_raw());
        self.check_err().map(|()| ret)
    }

    #[inline]
    fn check_err(&mut self) -> Result<()> {
        let kind = match unsafe { sys::err_code(self.as_raw()) } {
            sys::err_NIX_OK => return Ok(()),
            sys::err_NIX_ERR_UNKNOWN => ErrorKind::Unknown,
            sys::err_NIX_ERR_OVERFLOW => ErrorKind::Overflow,
            sys::err_NIX_ERR_KEY => ErrorKind::Key,
            sys::err_NIX_ERR_NIX_ERROR => ErrorKind::Nix,
            other => unreachable!("invalid error code: {other}"),
        };
        let mut err_msg_len = 0;
        let err_msg_ptr = unsafe {
            sys::err_msg(ptr::null_mut(), self.as_raw(), &mut err_msg_len)
        };
        let bytes = unsafe {
            slice::from_raw_parts(
                err_msg_ptr as *const u8,
                (err_msg_len + 1) as usize,
            )
        };
        let err_msg = unsafe { CStr::from_bytes_with_nul_unchecked(bytes) };
        Err(Error::new(kind, err_msg.to_owned()))
    }

    #[inline]
    fn with_primop_ptr<P: PrimOp, T>(
        &mut self,
        namespace: impl Namespace,
        fun: impl FnOnce(&mut Self, NonNull<sys::PrimOp>) -> T,
    ) -> Result<T> {
        // TODO: alloc() is implemented by leaking, so calling this repeatedly
        // will cause memory leaks. Fix this.
        let primop_raw =
            self.with_ptr(|ctx| unsafe { P::alloc(namespace.display(), ctx) })?;

        let primop_ptr = NonNull::new(primop_raw).ok_or_else(|| {
            Error::new(
                ErrorKind::Overflow,
                c"failed to allocate PrimOp for {primop:?}",
            )
        })?;

        let ret = fun(self, primop_ptr);

        self.with_raw(|ctx| unsafe {
            sys::gc_decref(ctx, primop_ptr.as_ptr().cast());
        })?;

        Result::Ok(ret)
    }
}
