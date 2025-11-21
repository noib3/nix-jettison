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
        attrset! {
            count: 42,
            enabled: true,
            nested: attrset! {
                { <Double as PrimOp>::NAME }: Double,
            },
            message: c"Hello from Rust!",
        }
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
