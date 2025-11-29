//! TODO: docs.

use core::ptr::NonNull;

use nix_bindings_sys as sys;

use crate::context::Context;
use crate::error::Result;
use crate::value::{NixValue, TryFromValue, Value, ValueKind};

/// TODO: docs.
pub struct Thunk<'a, V> {
    state: ThunkState<'a, V>,
}

enum ThunkState<'a, V> {
    Unevaluated(NixValue<'a>),
    Evaluated(V),
}

impl<'a, V> Thunk<'a, V> {
    /// TODO: docs.
    #[inline]
    pub fn force(self, ctx: &mut Context) -> Result<V>
    where
        V: TryFromValue<NixValue<'a>>,
    {
        match self.state {
            ThunkState::Unevaluated(value) => {
                ctx.force(value.as_ptr())?;
                V::try_from_value(value, ctx)
            },
            ThunkState::Evaluated(value) => Ok(value),
        }
    }
}

impl<'a, V: TryFromValue<NixValue<'a>>> TryFromValue<NixValue<'a>>
    for Thunk<'a, V>
{
    #[inline]
    fn try_from_value(value: NixValue<'a>, ctx: &mut Context) -> Result<Self> {
        let state = match value.kind() {
            ValueKind::Thunk => ThunkState::Unevaluated(value),
            _ => V::try_from_value(value, ctx).map(ThunkState::Evaluated)?,
        };
        Ok(Self { state })
    }
}

impl<V: Value> Value for Thunk<'_, V> {
    #[inline]
    fn kind(&self) -> ValueKind {
        match &self.state {
            ThunkState::Unevaluated(_) => ValueKind::Thunk,
            ThunkState::Evaluated(v) => v.kind(),
        }
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        ctx: &mut Context,
    ) -> Result<()> {
        match &self.state {
            ThunkState::Unevaluated(v) => unsafe { v.write(dest, ctx) },
            ThunkState::Evaluated(v) => unsafe { v.write(dest, ctx) },
        }
    }
}
