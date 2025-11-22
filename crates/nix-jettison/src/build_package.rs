use std::path::PathBuf;

use nix_bindings::prelude::*;

/// Builds a Rust package.
#[derive(nix_bindings::PrimOp)]
pub(crate) struct BuildPackage;

#[derive(nix_bindings::Args)]
pub(crate) struct SingleArg {
    args: BuildPackageArgs,
}

struct BuildPackageArgs {
    pkgs: AnyAttrset,
    src: PathBuf,
}

impl TryFromValue for BuildPackageArgs {
    #[inline]
    unsafe fn try_from_value(
        _value: core::ptr::NonNull<nix_bindings::sys::Value>,
        _ctx: &mut Context,
    ) -> Result<Self> {
        todo!();
    }
}

impl Function for BuildPackage {
    type Args = SingleArg;

    fn call(
        SingleArg { args }: Self::Args,
        _ctx: &mut Context,
    ) -> impl Value + use<> {
        attrset! {
            pkgs_len: args.pkgs.len(),
            src: args.src,
        }
    }
}
