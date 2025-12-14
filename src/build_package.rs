use core::result::Result;
use std::ffi::CString;
use std::path::Path;

use compact_str::CompactString;
use either::Either;
use nix_bindings::prelude::{Error as NixError, *};

use crate::resolve_build_graph::{
    BuildGraph,
    ResolveBuildGraphArgs,
    ResolveBuildGraphError,
};
use crate::vendor_deps::{VendorDeps, VendorDepsError, VendoredSources};

/// Builds a Rust package.
#[derive(nix_bindings::PrimOp)]
pub(crate) struct BuildPackage;

#[derive(nix_bindings::Args, nix_bindings::TryFromValue)]
#[args(flatten, name = "args")]
#[try_from(rename_all = camelCase)]
pub(crate) struct BuildPackageArgs<'a> {
    /// The package's name.
    package: CompactString,

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
    #[try_from(default = true)]
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

impl<'a> BuildPackageArgs<'a> {
    /// Returns the `buildRustCrate` function to use for building Rust crates.
    fn build_rust_crate(
        &self,
        ctx: &mut Context,
    ) -> Result<impl Callable + use<'a>, NixError> {
        let default_build_rust_crate =
            self.pkgs.get::<NixFunctor>(c"buildRustCrate", ctx)?;

        let mut build_rust_crate = Either::Right(default_build_rust_crate);

        // NOTE: ideally we would just include the crateOverrides in the global
        // arguments passed to `buildRustCrate`, but because upstream has a bug
        // where the crateOverrides are not included in the `processedAttrs`,
        // they end up leaking through to the attributes given to
        // `mkDerivation`, which causes an error.
        //
        // To get around that we need to override `buildRustCrate`.
        if let Some(crate_overrides) = self.crate_overrides {
            let apply_crate_overrides = ctx.eval::<NixLambda>(
                c"{ buildRustCrate, crateOverrides }:
                buildRustCrate.override { defaultCrateOverrides = crateOverrides; }",
            )?;

            let wrapped = apply_crate_overrides
                .call(
                    attrset! {
                        buildRustCrate: build_rust_crate,
                        crateOverrides: crate_overrides,
                    },
                    ctx,
                )?
                .force_into::<NixFunctor>(ctx)?;

            build_rust_crate = Either::Right(wrapped);
        }

        if let Some(global_overrides) = self.global_overrides {
            let apply_global_overrides = ctx.eval::<NixLambda>(
                c"{ buildRustCrate, globalOverrides }:
                args: (buildRustCrate args).overrideAttrs globalOverrides",
            )?;

            let wrapped = apply_global_overrides
                .call(
                    attrset! {
                        buildRustCrate: build_rust_crate,
                        globalOverrides: global_overrides,
                    },
                    ctx,
                )?
                .force_into::<NixLambda>(ctx)?;

            build_rust_crate = Either::Left(wrapped);
        }

        Ok(build_rust_crate)
    }
}

impl Function for BuildPackage {
    type Args<'a> = BuildPackageArgs<'a>;

    fn call<'a: 'a>(
        args: Self::Args<'a>,
        ctx: &mut Context,
    ) -> Result<NixDerivation<'static>, BuildPackageError> {
        let cargo_lock =
            VendorDeps::read_cargo_lock(&args.src.join("Cargo.lock"))?;

        let vendored_sources =
            VendoredSources::new(&cargo_lock, args.pkgs, ctx)?;

        let build_rust_crate = args.build_rust_crate(ctx)?;

        let vendor_dir_drv = vendored_sources.to_dir(args.pkgs, ctx)?;

        vendor_dir_drv.realise(ctx)?;

        let resolve_build_graph_args = ResolveBuildGraphArgs {
            vendor_dir: vendor_dir_drv.out_path(ctx)?.into(),
            src: args.src,
            package: args.package,
            features: args.features,
            all_features: args.all_features,
            no_default_features: args.no_default_features,
        };

        let build_graph = BuildGraph::resolve(&resolve_build_graph_args)?;

        #[cfg(feature = "forbid-cargo")]
        let cargo_is_forbidden = ctx.builtins().throw(ctx).call(
            c"buildRustCrate should've received all the arguments it needs to \
             not use Cargo",
            ctx,
        )?;

        let global_args = attrset! {
            buildTests: args.build_tests,
            #[cfg(feature = "forbid-cargo")]
            cargo: cargo_is_forbidden,
            release: args.release,
        }
        .merge(args.rustc.map(|rustc| attrset! { rust: rustc }));

        let mut build_crates: Vec<Thunk<'static>> =
            Vec::with_capacity(build_graph.nodes.len());

        let make_path = ctx.builtins().path(ctx);

        for node in build_graph.nodes {
            let src = match node.local_source_path.as_deref() {
                Some(path) => {
                    let name = path.file_name().expect("path has a file name");
                    make_path.call(attrset! { path, name }, ctx)?
                },
                None => vendored_sources
                    .get(node.source_id())
                    .expect("source is not local, so it must've been vendored"),
            };

            let args = attrset! { src }
                .merge(node.args)
                .merge(node.dependencies.map(|idx| build_crates[idx]))
                .merge(Attrset::borrow(&global_args));

            let build_crate_drv = build_rust_crate.call(args, ctx)?;

            build_crates.push(build_crate_drv);
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
        match err {
            BuildPackageError::Nix(nix_err) => nix_err,
            other => {
                let message = CString::new(other.to_string())
                    .expect("the Display impl doesn't contain any NUL bytes");
                Self::new(ErrorKind::Nix, message)
            },
        }
    }
}
