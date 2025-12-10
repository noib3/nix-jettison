use core::result::Result;
use std::ffi::CString;
use std::path::Path;

use nix_bindings::prelude::{Error as NixError, *};

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
    /// The package's name.
    package: String,

    /// The package set to use.
    pkgs: NixAttrset<'a>,

    /// The path to the root of the workspace the package is in.
    src: &'a Path,

    /// Whether to enable all features (equivalent to calling Cargo with the
    /// `--all-features` CLI flag).
    #[try_from(default)]
    all_features: bool,

    /// TODO: docs.
    #[try_from(default)]
    build_tests: bool,

    /// TODO: docs.
    #[try_from(default)]
    crate_overrides: Option<NixAttrset<'a>>,

    /// The list of the package's features to enable.
    #[try_from(default)]
    features: Vec<String>,

    /// TODO: docs.
    #[try_from(default)]
    global_overrides: Option<NixAttrset<'a>>,

    /// Whether to disable the default features (equivalent to calling Cargo
    /// with the `--no-default-features` CLI flag).
    #[try_from(default)]
    no_default_features: bool,

    /// TODO: docs.
    #[try_from(default)]
    release: bool,

    /// TODO: docs.
    #[try_from(default)]
    rustc: Option<NixDerivation<'a>>,
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

        let default_build_rust_crate =
            args.pkgs.get::<NixFunctor>(c"buildRustCrate", ctx)?;

        let build_rust_crate = match args.global_overrides {
            Some(_attrs) => todo!(),
            None => default_build_rust_crate,
        };

        let cargo_is_banned = ctx.builtins().throw(ctx).call(
            c"buildRustCrate should've received all the arguments it needs to \
             not use Cargo",
            ctx,
        )?;

        let global_args = attrset! {
            build_tests: args.build_tests,
            cargo: cargo_is_banned,
            crate_overrides: args.crate_overrides,
            release: args.release,
            rustc: args.rustc,
        };

        let mut build_crates: Vec<Thunk<'static>> =
            Vec::with_capacity(build_graph.crates.len());

        for args in build_graph.crates {
            let args = args.map_deps(|graph_idx| build_crates[graph_idx]);

            build_crates.push(build_rust_crate.call(
                args.to_attrset().merge(Attrset::borrow(&global_args)),
                ctx,
            )?);
        }

        // The derivation for the requested package is the root of the build
        // graph, which is the last element in the vector.
        build_crates
            .into_iter()
            .next_back()
            .expect("build graph is never empty")
            .force(ctx)
            .map_err(Into::into)
    }
}

impl From<BuildPackageError> for NixError {
    fn from(err: BuildPackageError) -> Self {
        let message = CString::new(err.to_string())
            .expect("the Display impl doesn't contain any NUL bytes");
        Self::new(ErrorKind::Nix, message)
    }
}
