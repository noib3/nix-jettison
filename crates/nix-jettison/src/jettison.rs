use nix_bindings::prelude::*;

use crate::build_package::BuildPackage;
use crate::vendor_deps::VendorDeps;

/// nix-jettison's library functions.
#[derive(nix_bindings::PrimOp)]
pub(crate) struct Jettison;

impl Constant for Jettison {
    fn value() -> impl Value {
        attrset! {
            { <BuildPackage as PrimOp>::NAME }: BuildPackage,
            { <VendorDeps as PrimOp>::NAME }: VendorDeps,
        }
    }
}
