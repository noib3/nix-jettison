use std::borrow::Cow;
use std::path::Path;

use nix_bindings::prelude::*;

/// Builds a Rust package.
#[derive(nix_bindings::PrimOp)]
pub(crate) struct BuildPackage;

#[derive(nix_bindings::Args, nix_bindings::TryFromValue)]
#[args(flatten, name = "args")]
pub(crate) struct BuildPackageArgs<'a> {
    pkgs: AnyAttrset<'a>,
    src: Cow<'a, Path>,
}

impl Function for BuildPackage {
    type Args<'a> = BuildPackageArgs<'a>;

    fn call<'a>(
        args: Self::Args<'a>,
        _: &mut Context,
    ) -> impl Value + use<'a> {
        attrset! {
            pkgs_len: args.pkgs.len(),
            src: args.src,
        }
    }
}
