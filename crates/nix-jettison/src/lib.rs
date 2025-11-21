#![allow(missing_docs)]

use nix_bindings::prelude::*;

/// nix-jettison's library functions.
struct Jettison;

impl PrimOp for Jettison {
    const NAME: &'static nix_bindings::Utf8CStr =
        // SAFETY: valid UTF-8.
        unsafe { nix_bindings::Utf8CStr::new_unchecked(c"jettison") };

    const DOCS: &'static core::ffi::CStr =
        c"nix-jettison's library functions.";
}

/// Doubles a number.
#[derive(Clone)]
struct Double;

impl PrimOp for Double {
    const NAME: &'static nix_bindings::Utf8CStr =
        // SAFETY: valid UTF-8.
        unsafe { nix_bindings::Utf8CStr::new_unchecked(c"double") };

    const DOCS: &'static core::ffi::CStr = c"Doubles a number.";
}

struct DoubleArgs {
    n: u8,
}

impl Args for DoubleArgs {
    const NAMES: &'static [*const core::ffi::c_char] =
        &[c"n".as_ptr(), core::ptr::null()];

    unsafe fn from_raw(
        args: core::ptr::NonNull<*mut nix_bindings::sys::Value>,
        ctx: &mut Context,
    ) -> Result<Self> {
        // SAFETY: up to caller
        let n = unsafe { ctx.get_arg::<u8>(args, 0)? };
        Ok(Self { n })
    }
}

impl PrimOpFun for Jettison {
    type Args = ();

    fn call(&self, _args: (), _: &mut Context) -> impl Value + use<> {
        let nested =
            LiteralAttrset::new(({ <Double as PrimOp>::NAME },), (Double,));

        LiteralAttrset::new(
            (
                // SAFETY: valid UTF-8.
                unsafe { nix_bindings::Utf8CStr::new_unchecked(c"count") },
                // SAFETY: valid UTF-8.
                unsafe { nix_bindings::Utf8CStr::new_unchecked(c"enabled") },
                // SAFETY: valid UTF-8.
                unsafe { nix_bindings::Utf8CStr::new_unchecked(c"nested") },
                // SAFETY: valid UTF-8.
                unsafe { nix_bindings::Utf8CStr::new_unchecked(c"message") },
            ),
            (42, true, nested, "Hello from Rust!"),
        )
    }
}

impl PrimOpFun for Double {
    type Args = DoubleArgs;

    fn call(&self, args: DoubleArgs, _: &mut Context) -> u8 {
        args.n * 2
    }
}

#[nix_bindings::entry]
fn jettison(ctx: &mut Context<Entrypoint>) {
    ctx.register_primop(Jettison)
}
