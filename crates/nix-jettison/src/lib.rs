#![allow(missing_docs)]

mod build_package;

use build_package::BuildPackage;
use nix_bindings::prelude::*;

/// nix-jettison's library functions.
#[derive(nix_bindings::PrimOp)]
struct Jettison;

impl Constant for Jettison {
    fn value() -> impl Value {
        attrset! {
            { <BuildPackage as PrimOp>::NAME }: BuildPackage,
        }
    }
}

#[nix_bindings::entry]
fn jettison(ctx: &mut Context<Entrypoint>) {
    ctx.register_primop::<Jettison>()
}
