use std::ffi::CString;

use cargo::core::compiler::CompileTarget;
use either::Either;
use nix_bindings::prelude::*;

use crate::build_node_args::BuildNodeArgs;

/// All the arguments needed to create the attribute set given to
/// `stdenv.mkDerivation` to build a single node in the build graph.
pub(crate) struct MakeDerivationArgs<'args, Deps, Src> {
    /// The
    /// [`BuildPackageArgs::crate_overrides`](crate::build_package::BuildPackageArgs::crate_overrides) field.
    pub(crate) crate_overrides: Option<NixAttrset<'args>>,

    /// The list of dependencies needed to build the node.
    ///
    /// This must be an iterator over derivations created with `mkDerivation
    /// args`, where `args` was an instance of `Self`.
    pub(crate) dependencies: Deps,

    /// The
    /// [`BuildPackageArgs::global_overrides`](crate::build_package::BuildPackageArgs::global_overrides) field.
    pub(crate) global_overrides: Option<NixAttrset<'args>>,

    /// The arguments coming from the workspace resolution step.
    pub(crate) node_args: BuildNodeArgs,

    /// Whether the node should be built in release mode.
    pub(crate) release: bool,

    /// The `rustc` derivation to include in the derivation's `buildInputs`.
    pub(crate) rustc: NixDerivation<'args>,

    /// The derivation pointing to the node's source code.
    pub(crate) src: Src,

    /// A handle to Nixpkgs's standard build environment.
    pub(crate) stdenv: NixAttrset<'args>,

    /// The compilation `--target` to pass to `rustc`, if any.
    ///
    /// This should only be set when cross-compiling.
    pub(crate) target: Option<CompileTarget>,
}

impl<'this, 'dep, Src, Deps> MakeDerivationArgs<'this, Deps, Src>
where
    Src: Value,
    Deps: Iterator<Item = NixDerivation<'dep>> + Clone,
{
    /// Converts `self` into the final attribute set given to
    /// `stdenv.mkDerivation`.
    pub(crate) fn into_attrs(
        self,
        ctx: &mut Context,
    ) -> Result<impl Attrset + Value + use<'this, 'dep, Src, Deps>> {
        let base_args = attrset! {};

        let args = base_args.merge(self.global_overrides);

        let Some(crate_overrides) = self.crate_overrides else {
            return Ok(Either::Left(args));
        };

        let package_name_cstr = CString::new(&*self.node_args.package_name)
            .expect("package name doesn't contain NUL bytes");

        let Some(override_fun) =
            crate_overrides.get_opt::<NixLambda>(&*package_name_cstr, ctx)?
        else {
            return Ok(Either::Left(args));
        };

        let overrides = override_fun
            .call(Value::borrow(&args), ctx)?
            .force_into::<NixAttrset>(ctx)?;

        Ok(Either::Right(args.merge(overrides)))
    }
}
