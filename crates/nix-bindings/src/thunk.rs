//! TODO: docs.

use core::ptr::NonNull;

use nix_bindings_sys as sys;

use crate::context::Context;
use crate::error::Result;
use crate::namespace::Namespace;
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
            ThunkState::Unevaluated(mut value) => {
                value.force_inline(ctx)?;
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
    fn force_inline(&mut self, ctx: &mut Context) -> Result<()> {
        if let ThunkState::Unevaluated(value) = &mut self.state {
            value.force_inline(ctx)?;
        }
        Ok(())
    }

    #[inline]
    fn kind(&self) -> ValueKind {
        match &self.state {
            // NOTE: even if the state is Unevaluated, we still call kind() on
            // the inner value instead of always returning ValueKind::Thunk
            // because a previous call to 'force_inline()' may have changed the
            // value's kind.
            ThunkState::Unevaluated(v) => v.kind(),
            ThunkState::Evaluated(v) => v.kind(),
        }
    }

    #[inline]
    unsafe fn write(
        &self,
        dest: NonNull<sys::Value>,
        namespace: impl Namespace,
        ctx: &mut Context,
    ) -> Result<()> {
        match &self.state {
            ThunkState::Unevaluated(v) => unsafe {
                v.write(dest, namespace, ctx)
            },
            ThunkState::Evaluated(v) => unsafe {
                v.write(dest, namespace, ctx)
            },
        }
    }
}
