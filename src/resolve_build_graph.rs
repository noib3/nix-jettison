use core::result::Result;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::{env, io};

use cargo::core::compiler::{CompileKind, RustcTargetData};
use cargo::core::profiles::Profiles;
use cargo::core::resolver::{CliFeatures, ForceAllTargets, HasDevUnits};
use cargo::core::{
    Dependency,
    MaybePackage,
    Package,
    PackageId,
    Shell,
    Workspace,
};
use cargo::{GlobalContext, ops};
use compact_str::CompactString;
use nix_bindings::prelude::{Error as NixError, *};

use crate::build_graph::BuildGraph;

/// Resolves the build graph of a Rust package.
#[derive(nix_bindings::PrimOp)]
pub(crate) struct ResolveBuildGraph;

#[derive(nix_bindings::Args, nix_bindings::TryFromValue)]
#[args(flatten, name = "args")]
#[try_from(rename_all = camelCase)]
pub(crate) struct ResolveBuildGraphArgs<'a> {
    /// The path to the root of the workspace the package is in.
    pub(crate) src: &'a Path,

    /// The derivation for the directory containing all the vendored
    /// dependencies.
    ///
    /// This can be obtained by calling `jettison.vendorDeps { ... }`.
    pub(crate) vendor_dir: NixDerivation<'a>,

    /// Whether to enable all features (equivalent to calling Cargo with the
    /// `--all-features` CLI flag).
    #[try_from(default)]
    pub(crate) all_features: bool,

    /// The list of the package's features to enable.
    #[try_from(default)]
    pub(crate) features: Vec<String>,

    /// Whether to disable the default features (equivalent to calling Cargo
    /// with the `--no-default-features` CLI flag).
    #[try_from(default)]
    pub(crate) no_default_features: bool,

    /// The package's name.
    #[try_from(default)]
    pub(crate) package: Option<CompactString>,

    /// The profile to use when building the package.
    #[try_from(default = CompactString::const_new("release"))]
    pub(crate) profile: CompactString,
}

pub(crate) struct WorkspaceResolve<'ws> {
    inner: ops::WorkspaceResolve<'ws>,
    package_id: PackageId,
    profiles: Profiles,
    target_data: RustcTargetData<'ws>,
    target: CompileKind,
    workspace: Workspace<'ws>,
}

/// The type of error that can occur when resolving a build graph fails.
#[derive(Debug, derive_more::Display, cauchy::From)]
#[display("{_0}")]
pub(crate) enum ResolveBuildGraphError {
    /// Configuring the global Cargo context failed.
    ConfigureCargoContext(anyhow::Error),

    /// Constructing the [`RustcTargetData`] failed.
    CreateTargetData(anyhow::Error),

    /// Constructing the [`Workspace`] failed.
    CreateWorkspace(anyhow::Error),

    /// Getting the current working directory failed.
    #[display("couldn't get the current directory of the process: {_0}")]
    GetCwd(io::Error),

    /// The `package` argument provided by the user didn't match the name of
    /// any package in the workspace.
    #[display("no package named '{_0}' found in the workspace")]
    InvalidPackageName(CompactString),

    /// A Nix runtime error occurred.
    Nix(#[from] NixError),

    /// Parsing the features failed.
    ParseFeatures(anyhow::Error),

    /// Creating the [`Profiles`] failed.
    ResolveProfiles(anyhow::Error),

    /// Resolving the [`Workspace`] failed.
    ResolveWorkspace(anyhow::Error),

    /// The user didn't specify a package name, and the workspace manifest is a
    /// virtual manifest with no root package.
    #[display(
        "couldn't determine the root package: no `package` was set, and the \
         workspace has a virtual manifest with no root package'"
    )]
    VirtualManifestNoRootPackage,
}

impl ResolveBuildGraphArgs<'_> {
    fn compile_target(&self) -> Result<CompileKind, ResolveBuildGraphError> {
        Ok(CompileKind::Host)
    }

    fn features(&self) -> Result<CliFeatures, ResolveBuildGraphError> {
        CliFeatures::from_command_line(
            &self.features,
            self.all_features,
            !self.no_default_features,
        )
        .map_err(ResolveBuildGraphError::ParseFeatures)
    }
}

impl<'ws> WorkspaceResolve<'ws> {
    pub(crate) fn deps(
        &self,
        pkg_id: PackageId,
    ) -> impl Iterator<Item = (PackageId, &Dependency)> {
        self.inner.targeted_resolve.deps(pkg_id).flat_map(
            |(dep_pkg_id, dep_set)| {
                // Filter out dependencies that don't match our target
                // platform.
                let deps = dep_set.iter().filter(|&dep| {
                    self.target_data.dep_platform_activated(dep, self.target)
                });

                deps.map(move |dep| (dep_pkg_id, dep))
            },
        )
    }

    pub(crate) fn features(
        &self,
        pkg_id: PackageId,
    ) -> impl Iterator<Item = &str> {
        self.inner.targeted_resolve.features(pkg_id).iter().map(|s| s.as_str())
    }

    pub(crate) fn package(&self, pkg_id: PackageId) -> Option<&Package> {
        self.inner.pkg_set.get_one(pkg_id).ok()
    }

    pub(crate) fn profiles(&self) -> &Profiles {
        &self.profiles
    }

    /// The [`CompileKind`] for the root of the build graph.
    pub(crate) fn root_compile_kind(&self) -> CompileKind {
        self.target
    }

    /// The [`PackageId`] of the package at the root of the build graph.
    pub(crate) fn root_id(&self) -> &PackageId {
        &self.package_id
    }

    pub(crate) fn target_data(&self) -> &RustcTargetData<'ws> {
        &self.target_data
    }

    pub(crate) fn workspace(&self) -> &Workspace<'ws> {
        &self.workspace
    }

    fn new(
        workspace: Workspace<'ws>,
        package_id: PackageId,
        args: &ResolveBuildGraphArgs,
    ) -> Result<Self, ResolveBuildGraphError> {
        let target = args.compile_target()?;

        let mut target_data = RustcTargetData::new(&workspace, &[target])
            .map_err(ResolveBuildGraphError::CreateTargetData)?;

        let inner = ops::resolve_ws_with_opts(
            &workspace,
            &mut target_data,
            &[target],
            &args.features()?,
            &[package_id.to_spec()],
            HasDevUnits::No,
            ForceAllTargets::No,
            true,
        )
        .map_err(ResolveBuildGraphError::ResolveWorkspace)?;

        let profiles = Profiles::new(&workspace, args.profile.as_str().into())
            .map_err(ResolveBuildGraphError::ResolveProfiles)?;

        Ok(Self { inner, package_id, profiles, target_data, target, workspace })
    }
}

impl Function for ResolveBuildGraph {
    type Args<'a> = ResolveBuildGraphArgs<'a>;

    fn call<'a: 'a>(
        args: Self::Args<'a>,
        ctx: &mut Context,
    ) -> Result<BuildGraph, ResolveBuildGraphError> {
        let manifest_path = args.src.join("Cargo.toml");

        args.vendor_dir.realise(ctx)?;

        let cargo_ctx =
            cargo_ctx(args.vendor_dir.out_path(ctx)?.join(".cargo"))?;

        let workspace = Workspace::new(&manifest_path, &cargo_ctx)
            .map_err(ResolveBuildGraphError::CreateWorkspace)?;

        let package =
            match args.package.as_deref() {
                Some(package_name) => workspace
                    .members()
                    .find(|package| package.name() == package_name)
                    .ok_or_else(|| {
                        ResolveBuildGraphError::InvalidPackageName(
                            package_name.into(),
                        )
                    })?,

                None => match workspace.root_maybe() {
                    MaybePackage::Package(package) => package,
                    MaybePackage::Virtual(_) => return Err(
                        ResolveBuildGraphError::VirtualManifestNoRootPackage,
                    ),
                },
            };

        let package_id = package.package_id();

        let resolve = WorkspaceResolve::new(workspace, package_id, &args)?;

        Ok(BuildGraph::new(package_id, &resolve))
    }
}

impl From<ResolveBuildGraphError> for NixError {
    fn from(err: ResolveBuildGraphError) -> Self {
        match err {
            ResolveBuildGraphError::Nix(nix_err) => nix_err,
            other => {
                let message = CString::new(other.to_string())
                    .expect("the Display impl doesn't contain any NUL bytes");
                Self::new(ErrorKind::Nix, message)
            },
        }
    }
}

fn cargo_ctx(
    cargo_home: PathBuf,
) -> Result<GlobalContext, ResolveBuildGraphError> {
    let shell = Shell::new();

    let cwd = env::current_dir().map_err(ResolveBuildGraphError::GetCwd)?;

    let mut ctx = GlobalContext::new(shell, cwd, cargo_home);

    ctx.configure(0, false, None, true, true, true, &None, &[], &[])
        .map_err(ResolveBuildGraphError::ConfigureCargoContext)?;

    Ok(ctx)
}
