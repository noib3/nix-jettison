use std::path::Path;

use nix_bindings::prelude::*;

/// Builds a Rust package.
#[derive(nix_bindings::PrimOp)]
pub(crate) struct BuildPackage;

#[derive(nix_bindings::Args)]
#[args(flatten, name = "args")]
pub(crate) struct BuildPackageArgs<'a> {
    pkgs: AnyAttrset<'a>,
    src: &'a Path,
}

impl<'a> TryFromValue<'a> for BuildPackageArgs<'a> {
    #[inline]
    unsafe fn try_from_value(
        value: ValuePointer<'a>,
        ctx: &mut Context,
    ) -> Result<Self> {
        // SAFETY: up to the caller.
        let attrset = unsafe { AnyAttrset::try_from_value(value, ctx)? };
        let pkgs = attrset.get(c"pkgs", ctx)?;
        let src = attrset.get(c"src", ctx)?;
        Ok(Self { pkgs, src })
    }
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
