//! TODO: docs.

use core::ptr::NonNull;

use nix_bindings_sys as sys;

use crate::context::Context;
use crate::error::Result;
use crate::namespace::Namespace;
use crate::value::{NixValue, TryFromValue, Value, ValueKind};

/// TODO: docs.
pub trait Lazy<Output> {
    /// TODO: docs.
    fn force(self, ctx: &mut Context) -> Result<Output>;
}

/// TODO: docs.
pub struct Thunk<'a> {
    value: NixValue<'a>,
}

impl<'a> Thunk<'a> {
    /// TODO: docs.
    #[inline(always)]
    pub fn force_into<T>(self, ctx: &mut Context) -> Result<T>
    where
        T: TryFromValue<NixValue<'a>>,
    {
        self.into_lazy::<T>().force(ctx)
    }

    /// TODO: docs.
    #[inline(always)]
    pub fn into_lazy<T>(self) -> impl Lazy<T>
    where
        T: TryFromValue<NixValue<'a>>,
    {
        self
    }

    #[inline(always)]
    pub(crate) fn new(value: NixValue<'a>) -> Self {
        Self { value }
    }
}

impl Value for Thunk<'_> {
    #[inline]
    fn force_inline(&mut self, ctx: &mut Context) -> Result<()> {
        self.value.force_inline(ctx)
    }

    #[inline]
    fn kind(&self) -> ValueKind {
        self.value.kind()
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        unsafe { self.value.write(dest, namespace, ctx) }
    }
}

impl<'a> TryFromValue<NixValue<'a>> for Thunk<'a> {
    #[inline]
    fn try_from_value(value: NixValue<'a>, _: &mut Context) -> Result<Self> {
        Ok(Self::new(value))
    }
}

impl<'a, T: TryFromValue<NixValue<'a>>> Lazy<T> for Thunk<'a> {
    #[inline]
    fn force(mut self, ctx: &mut Context) -> Result<T> {
        self.value.force_inline(ctx)?;
        T::try_from_value(self.value, ctx)
    }
}

#[cfg(feature = "either")]
impl<L, R, T> Lazy<T> for either::Either<L, R>
where
    L: Lazy<T>,
    R: Lazy<T>,
{
    #[inline]
    fn force(self, ctx: &mut Context) -> Result<T> {
        match self {
            Self::Left(l) => l.force(ctx),
            Self::Right(r) => r.force(ctx),
        }
    }
}
