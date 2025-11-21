#![allow(missing_docs)]

use nix_bindings::prelude::*;

/// nix-jettison's library functions.
#[derive(nix_bindings::PrimOp)]
struct Jettison;

/// Doubles a number.
#[derive(nix_bindings::PrimOp)]
struct Double;

#[derive(nix_bindings::Args)]
struct DoubleArgs {
    n: i32,
}

impl Constant for Jettison {
    fn value() -> impl Value {
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
            (42, true, nested, c"Hello from Rust!"),
        )
    }
}

impl Function for Double {
    type Args = DoubleArgs;

    fn call(args: DoubleArgs, _: &mut Context) -> i32 {
        args.n * 2
    }
}

#[nix_bindings::entry]
fn jettison(ctx: &mut Context<Entrypoint>) {
    ctx.register_primop::<Jettison>()
}
