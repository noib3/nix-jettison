use core::fmt::Write;
use core::iter;
use std::ffi::CString;

use cargo::core::compiler::CompileTarget;
use either::Either;
use nix_bindings::prelude::*;

use crate::build_node_args::{BuildNodeArgs, CrateType, DerivationType};

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
    pub(crate) node_args: &'args BuildNodeArgs,

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
    Deps: Iterator<Item = (&'dep BuildNodeArgs, NixDerivation<'dep>)> + Clone,
{
    /// Converts `self` into the final attribute set given to
    /// `stdenv.mkDerivation`.
    pub(crate) fn into_attrs(
        self,
        ctx: &mut Context,
    ) -> Result<impl Attrset + Value + use<'this, 'dep, Src, Deps>> {
        let base_args = attrset! {
            name: self.node_args.derivation_name(),
            buildInputs: [self.rustc].into_value(),
            nativeBuildInputs: <[NixDerivation; 0]>::default().into_value(),
            configurePhase: self.configure_phase(ctx)?,
            buildPhase: self.build_phase(ctx)?,
            installPhase: self.install_phase(ctx)?,
            dontStrip: true,
            // See https://github.com/NixOS/nixpkgs/issues/218712.
            stripExclude: [ c"*.rlib" ].into_value(),
            version: &*self.node_args.version,
            src: self.src,
        };

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

    fn configure_phase(&self, _ctx: &mut Context) -> Result<String> {
        // ## Build scripts
        // 1: set up environment variables during configurePhase;
        // 2: if the package has a build script, we need to source its output
        //    `env` file during the configurePhase as well;
        // 3: for build scripts, let's first pretend we don't have to set
        //    any environment variables coming from the build scripts of other
        //    dependencies;
        // 4: if the package has a build script, we may need to include any
        //    files that have been generated and placed in the `$OUT_DIR`;
        // 5: if the package has a build script, we should run it, place its
        //    stdout in a file, then run a program that parses the file and
        //    produces a `env` file.
        //
        // ## Native libraries
        // 1: get the list of native libraries from somewhere (I'm assuming the
        //    native_build_inputs?);
        // 2: for every native library, add a `-C
        //    link-arg={full_path_to_{*.so|*.dylib|*.a}}` argument to the flags
        //    given to `rustc`
        // 3: there may be other linker flags coming from build scripts, but
        //    those should be taken care of by the program that parses its
        //    output, so that the only thing we might have to do is pass
        //    `$EXTRA_RUSTC_ARGS` to the `rustc` calls;
        let mut configure_phase = "runHook preConfigure".to_string();
        configure_phase.push_str("\nrunHook postConfigure");
        Ok(configure_phase)
    }

    fn build_phase(&self, ctx: &mut Context) -> Result<String> {
        let crate_types = match &self.node_args.r#type {
            DerivationType::Bin(bins) => {
                Either::Right(bins.iter().map(CrateType::Bin))
            },
            DerivationType::Lib(lib) => {
                Either::Left(iter::once(CrateType::Lib(&lib)))
            },
            DerivationType::BuildScript(path) => {
                Either::Left(iter::once(CrateType::BuildScript(path)))
            },
        };

        let mut build_phase = "runHook preBuild".to_owned();

        for crate_type in crate_types {
            build_phase.push_str("\nrustc");

            for rustc_arg in self.node_args.build_rustc_args(
                self.release,
                crate_type,
                self.dependencies.clone(),
                self.target.as_ref(),
                ctx,
            ) {
                build_phase.push(' ');
                build_phase.push_str(rustc_arg.as_ref());
            }
        }

        build_phase.push_str("\nrunHook postBuild");

        Ok(build_phase)
    }

    fn install_phase(&self, _ctx: &mut Context) -> Result<String> {
        let mut install_phase = "runHook preInstall".to_owned();
        install_phase.push_str("\nmkdir -p $out");
        write!(&mut install_phase, "cp -r {}/* $out", self.node_args.out_dir())
            .expect("writing to string can't fail");
        install_phase.push_str("\nrunHook postInstall");
        Ok(install_phase)
    }
}
