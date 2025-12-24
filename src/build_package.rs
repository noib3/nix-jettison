use core::result::Result;
use std::ffi::CString;
use std::path::Path;

use compact_str::CompactString;
use nix_bindings::prelude::{Error as NixError, *};

use crate::build_graph::BuildGraph;
use crate::make_derivation::{
    self,
    DerivationType,
    make_deps,
    make_derivation,
};
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
    pub(crate) pkgs: NixAttrset<'a>,

    /// The path to the root of the workspace the package is in.
    src: &'a Path,

    /// Whether to enable all features (equivalent to calling Cargo with the
    /// `--all-features` CLI flag).
    #[try_from(default)]
    all_features: bool,

    /// TODO: docs.
    #[try_from(default)]
    pub(crate) crate_overrides: Option<NixAttrset<'a>>,

    /// The list of the package's features to enable.
    #[try_from(default)]
    features: Vec<String>,

    /// TODO: docs.
    #[try_from(default)]
    pub(crate) global_overrides: Option<NixAttrset<'a>>,

    /// Whether to disable the default features (equivalent to calling Cargo
    /// with the `--no-default-features` CLI flag).
    #[try_from(default)]
    no_default_features: bool,

    /// The package's name.
    #[try_from(default)]
    package: Option<CompactString>,

    /// TODO: docs.
    #[try_from(default = true)]
    pub(crate) release: bool,

    /// TODO: docs.
    #[try_from(default)]
    pub(crate) rustc: Option<NixDerivation<'a>>,
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

impl BuildPackage {
    fn get_build_graph(
        args: BuildPackageArgs,
        ctx: &mut Context,
    ) -> Result<BuildGraph, BuildPackageError> {
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

        <ResolveBuildGraph as Function>::call(resolve_build_graph_args, ctx)
            .map_err(Into::into)
    }
}

impl Function for BuildPackage {
    type Args<'a> = BuildPackageArgs<'a>;

    #[expect(clippy::too_many_lines)]
    fn call<'a: 'a>(
        args: Self::Args<'a>,
        ctx: &mut Context,
    ) -> Result<NixDerivation<'static>, BuildPackageError> {
        let global_args = make_derivation::GlobalArgs::new(&args, ctx)?;

        let build_graph = Self::get_build_graph(args, ctx)?;

        let mut library_derivations: Vec<NixDerivation<'static>> =
            Vec::with_capacity(build_graph.nodes.len());

        for (node_idx, node) in build_graph.nodes.iter().enumerate() {
            let edges = &build_graph.edges[node_idx];

            let build_deps = edges.build_dependencies.iter().map(|&idx| {
                let node = &build_graph.nodes[idx];
                let drv = library_derivations[idx].clone();
                (node, drv)
            });

            let normal_deps = edges.dependencies.iter().map(|&idx| {
                let node = &build_graph.nodes[idx];
                let drv = library_derivations[idx].clone();
                (node, drv)
            });

            let all_direct_deps = build_deps
                .clone()
                .chain(normal_deps.clone())
                .map(|(_node, drv)| drv);

            let deps_drv = make_deps(
                &node.package_attrs,
                all_direct_deps,
                &global_args.mk_derivation,
                ctx,
            )?;

            let src = match node.package_src.as_deref() {
                Some(path) => {
                    let name = path.file_name().expect("path has a file name");
                    ctx.builtins()
                        .path(ctx)
                        .call(attrset! { path, name }, ctx)?
                },
                None => todo!(),
                // None => vendored_sources
                //     .get(node.source_id())
                //     .expect("source is not local, so it must've been vendored"),
            };

            let build_script = if let Some(build_script) = &node.build_script {
                Some(make_derivation(
                    DerivationType::BuildScript(build_script),
                    node,
                    src.clone(),
                    deps_drv.clone(),
                    build_deps,
                    &global_args,
                    ctx,
                )?)
            } else {
                None
            };

            let library = if let Some(library) = &node.library {
                Some(make_derivation(
                    DerivationType::Library { build_script, library },
                    node,
                    src.clone(),
                    deps_drv.clone(),
                    normal_deps.clone(),
                    &global_args,
                    ctx,
                )?)
            } else {
                None
            };

            let _binaries = if !node.binaries.is_empty() {
                Some(make_derivation(
                    DerivationType::Binaries {
                        build_script,
                        library,
                        binaries: &node.binaries,
                    },
                    node,
                    src,
                    deps_drv,
                    normal_deps,
                    &global_args,
                    ctx,
                )?)
            } else {
                None
            };

            if let Some(drv) = library {
                library_derivations.push(drv);
            }
        }

        // The derivation for the requested package is the root of the build
        // graph, which is the last element in the vector.
        Ok(library_derivations
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
