use nix_bindings_sys as sys;

use crate::Result;

/// TODO: docs.
pub trait Value: Sealed + Sized + 'static {
    /// Writes this value into the given, pre-allocated destination.
    #[doc(hidden)]
    unsafe fn write(
        self,
        dest: *mut sys::Value,
        ctx: &mut Context,
    ) -> Result<()>;
}

use sealed::Sealed;

use crate::Context;

mod sealed {
    pub trait Sealed {}
}
