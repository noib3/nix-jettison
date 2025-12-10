use core::result::Result;
use std::ffi::CString;
use std::path::Path;

use nix_bindings::prelude::{Error as NixError, *};

use crate::build_crate_args::BuildCrateArgs;
use crate::resolve_build_graph::{
    ResolveBuildGraph,
    ResolveBuildGraphArgs,
    ResolveBuildGraphError,
};
use crate::vendor_deps::{VendorDeps, VendorDepsArgs, VendorDepsError};

/// Builds a Rust package.
#[derive(nix_bindings::PrimOp)]
pub(crate) struct BuildPackage;

#[derive(nix_bindings::Args, nix_bindings::TryFromValue)]
#[args(flatten, name = "args")]
#[try_from(rename_all = camelCase)]
pub(crate) struct BuildPackageArgs<'a> {
    pkgs: NixAttrset<'a>,
    src: &'a Path,
    package: String,
    #[try_from(default)]
    features: Vec<String>,
    #[try_from(default)]
    all_features: bool,
    #[try_from(default)]
    no_default_features: bool,
}

/// The type of error that can occur when building a package fails.
#[derive(Debug, derive_more::Display, cauchy::From)]
#[display("{_0}")]
pub(crate) enum BuildPackageError {
    /// A Nix runtime error occurred.
    Nix(#[from] NixError),

    /// Resolving the build graph failed.
    ResolveBuildGraph(#[from] ResolveBuildGraphError),

    /// Vendoring the dependencies failed.
    VendorDeps(#[from] VendorDepsError),
}

impl Function for BuildPackage {
    type Args<'a> = BuildPackageArgs<'a>;

    fn call<'a: 'a>(
        args: Self::Args<'a>,
        ctx: &mut Context,
    ) -> Result<NixDerivation<'static>, BuildPackageError> {
        let vendor_deps_args = VendorDepsArgs {
            pkgs: args.pkgs,
            cargo_lock: args.src.join("Cargo.lock").into(),
        };

        let vendor_dir = <VendorDeps as Function>::call(vendor_deps_args, ctx)?;

        let resolve_build_graph_args = ResolveBuildGraphArgs {
            vendor_dir: vendor_dir.path().into(),
            src: args.src,
            package: args.package,
            features: args.features,
            all_features: args.all_features,
            no_default_features: args.no_default_features,
        };

        let build_graph = <ResolveBuildGraph as Function>::call(
            resolve_build_graph_args,
            ctx,
        )?;

        let build_rust_crate =
            args.pkgs.get::<NixFunctor>(c"buildRustCrate", ctx)?;

        let mut build_crates: Vec<NixDerivation<'static>> =
            Vec::with_capacity(build_graph.crates.len());

        for args in build_graph.crates {
            let args = BuildCrateArgs {
                required: args.required,
                optional: args.optional.map_deps(|idx| build_crates[idx]),
                global: args.global,
            };

            let derivation = build_rust_crate
                .call(args.as_into_value(), ctx)
                .map_err(BuildPackageError::Nix)?
                // FIXME: does forcing here disable build parallelism?
                .force(ctx)?;

            build_crates.push(derivation);
        }

        // The derivation for the requested package is the root of the build
        // graph, which is the last element in the list.
        Ok(build_crates
            .into_iter()
            .next_back()
            .expect("build graph is never empty"))
    }
}

impl From<BuildPackageError> for NixError {
    fn from(err: BuildPackageError) -> Self {
        let message = CString::new(err.to_string())
            .expect("the Display impl doesn't contain any NUL bytes");
        Self::new(ErrorKind::Nix, message)
    }
}
