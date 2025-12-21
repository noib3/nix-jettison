use core::result::Result;
use std::ffi::CString;
use std::path::Path;

use compact_str::CompactString;
use either::Either;
use nix_bindings::prelude::{Error as NixError, *};

use crate::resolve_build_graph::{
    ResolveBuildGraph,
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

    /// The package's name.
    #[try_from(default)]
    package: Option<CompactString>,

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

        let resolve_build_graph_args = ResolveBuildGraphArgs {
            src: args.src,
            vendor_dir: vendored_sources.to_dir(args.pkgs, ctx)?,
            all_features: args.all_features,
            features: args.features,
            no_default_features: args.no_default_features,
            package: args.package,
            profile: CompactString::const_new(if args.release {
                "release"
            } else {
                "dev"
            }),
        };

        let build_graph = <ResolveBuildGraph as Function>::call(
            resolve_build_graph_args,
            ctx,
        )?;

        let mk_derivation = args
            .pkgs
            .get::<NixAttrset>(c"stdenv", ctx)?
            .get::<NixLambda>(c"mkDerivation", ctx)?;

        let rustc = match args.rustc {
            Some(rustc) => rustc,
            None => args.pkgs.get::<NixDerivation>(c"rustc", ctx)?,
        };

        let build_inputs = &[rustc];

        let native_build_inputs = &[];

        let mut build_nodes: Vec<NixDerivation<'static>> =
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

            let dependencies =
                node.dependencies.iter().map(|&idx| build_nodes[idx].clone());

            let base_args = node
                .args
                .to_mk_derivation_args(
                    src,
                    build_inputs,
                    native_build_inputs,
                    dependencies,
                    args.release,
                    ctx,
                )
                .merge(args.global_overrides);

            let mk_derivation_args = if let Some(override_fun) = override_fun(
                &args.crate_overrides,
                &node.args.package_name,
                ctx,
            )? {
                let new_attrs = override_fun
                    .call(Value::borrow(&base_args), ctx)?
                    .force_into::<NixAttrset>(ctx)?;

                Either::Right(base_args.merge(new_attrs))
            } else {
                Either::Left(base_args)
            };

            let build_crate_drv =
                mk_derivation.call(mk_derivation_args, ctx)?.force_into(ctx)?;

            build_nodes.push(build_crate_drv);
        }

        // The derivation for the requested package is the root of the build
        // graph, which is the last element in the vector.
        Ok(build_nodes
            .into_iter()
            .next_back()
            .expect("build graph is never empty"))
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

/// Returns the function to call to override the `mkDerivation` arguments
/// for crates in the given package, if any.
fn override_fun<'a>(
    overrides: &Option<NixAttrset<'a>>,
    package_name: &str,
    ctx: &mut Context,
) -> Result<Option<NixLambda<'a>>, NixError> {
    let Some(overrides) = overrides else {
        return Ok(None);
    };

    let package_name_cstr = CString::new(package_name)
        .expect("package name doesn't contain NUL bytes");

    overrides.get_opt::<NixLambda>(&*package_name_cstr, ctx)
}
