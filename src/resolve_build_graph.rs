use core::result::Result;
use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::{env, io};

use cargo::core::compiler::{CompileKind, RustcTargetData};
use cargo::core::dependency::DepKind;
use cargo::core::profiles::Profiles;
use cargo::core::resolver::{CliFeatures, ForceAllTargets, HasDevUnits};
use cargo::core::{Dependency, MaybePackage, PackageId, Shell, Workspace};
use cargo::{GlobalContext, ops};
use compact_str::CompactString;
use nix_bindings::prelude::{Error as NixError, *};

use crate::build_crate_args::{BuildCrateArgs, Dependencies, SourceId};

/// Resolves the build graph of a Rust package.
#[derive(nix_bindings::PrimOp)]
pub(crate) struct ResolveBuildGraph;

#[derive(nix_bindings::Args, nix_bindings::TryFromValue)]
#[args(flatten, name = "args")]
#[try_from(rename_all = camelCase)]
pub(crate) struct ResolveBuildGraphArgs<'a> {
    /// The path to the root of the workspace the package is in.
    pub(crate) src: &'a Path,

    /// The path to the directory where dependencies have been vendored.
    ///
    /// This can be obtained by calling `jettison.vendorDeps { ... }`.
    #[try_from(with = get_vendor_dir)]
    pub(crate) vendor_dir: Cow<'a, Path>,

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

pub(crate) struct BuildGraph {
    pub(crate) nodes: Vec<BuildGraphNode<usize>>,
    pkg_id_to_idx: HashMap<PackageId, usize>,
}

pub(crate) struct BuildGraphNode<Dep> {
    pub(crate) args: BuildCrateArgs,
    pub(crate) dependencies: Dependencies<Dep>,
    pub(crate) local_source_path: Option<PathBuf>,
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
    ) -> impl Iterator<Item = (PackageId, impl Iterator<Item = &Dependency>)> + '_
    {
        self.inner.targeted_resolve.deps(pkg_id).map(|(dep_id, dep_set)| {
            (
                dep_id,
                // Filter out dependencies that don't match our target
                // platform.
                dep_set.iter().filter(|&dep| {
                    self.target_data.dep_platform_activated(dep, self.target)
                }),
            )
        })
    }

    pub(crate) fn features(
        &self,
        pkg_id: PackageId,
    ) -> impl Iterator<Item = &str> {
        self.inner.targeted_resolve.features(pkg_id).iter().map(|s| s.as_str())
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

    fn get_package(&self, pkg_id: PackageId) -> Option<&cargo::core::Package> {
        self.inner.pkg_set.get_one(pkg_id).ok()
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

impl BuildGraph {
    pub(crate) fn resolve(
        args: &ResolveBuildGraphArgs,
    ) -> Result<Self, ResolveBuildGraphError> {
        let manifest_path = args.src.join("Cargo.toml");

        let cargo_ctx = cargo_ctx(args.vendor_dir.join(".cargo"))?;

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

        let resolve = WorkspaceResolve::new(workspace, package_id, args)?;

        Self::new(package_id, &resolve)
    }

    fn build_recursive(
        this: &mut Self,
        pkg_id: PackageId,
        resolve: &WorkspaceResolve,
    ) -> usize {
        // Fast path if we've already seen this package.
        if let Some(&idx) = this.pkg_id_to_idx.get(&pkg_id) {
            return idx;
        }

        let mut dependencies = Dependencies::default();

        for (dep_id, dep_set) in resolve.deps(pkg_id) {
            // TODO: shouldn't this go inside the loop below? The dep_set is an
            // iterator bc the same dependency can be under `[dependencies]`,
            // `[build-dependencies]`, and `[dev-dependencies]`.
            let dep_idx = Self::build_recursive(this, dep_id, resolve);

            for dep in dep_set {
                match dep.kind() {
                    DepKind::Normal => dependencies.normal.push(dep_idx),
                    DepKind::Development => {},
                    DepKind::Build => dependencies.build.push(dep_idx),
                }
            }
        }

        let package = resolve
            .get_package(pkg_id)
            .expect("package ID not found in workspace");

        let build_crate_args = BuildGraphNode {
            args: BuildCrateArgs::new(package, resolve),
            dependencies,
            local_source_path: package
                .package_id()
                .source_id()
                .is_path()
                .then(|| package.root().to_owned()),
        };

        let idx = this.nodes.len();

        this.nodes.push(build_crate_args);
        this.pkg_id_to_idx.insert(pkg_id, idx);

        idx
    }

    fn new(
        root_id: PackageId,
        resolve: &WorkspaceResolve,
    ) -> Result<Self, ResolveBuildGraphError> {
        let mut this =
            Self { nodes: Vec::new(), pkg_id_to_idx: HashMap::new() };
        Self::build_recursive(&mut this, root_id, resolve);
        Ok(this)
    }
}

impl<Dep> BuildGraphNode<Dep> {
    pub(crate) fn source_id(&self) -> SourceId<'_> {
        self.args.source_id()
    }
}

impl Function for ResolveBuildGraph {
    type Args<'a> = ResolveBuildGraphArgs<'a>;

    fn call<'a: 'a>(
        args: Self::Args<'a>,
        _: &mut Context,
    ) -> Result<BuildGraph, ResolveBuildGraphError> {
        BuildGraph::resolve(&args)
    }
}

impl<Dep: Value> ToValue for BuildGraphNode<Dep> {
    fn to_value<'a>(&'a self, _: &mut Context) -> impl Value + use<'a, Dep> {
        Attrset::borrow(&self.args)
            .merge(Attrset::borrow(&self.dependencies))
            .merge(self.local_source_path.as_deref().map(|path| {
                attrset! {
                    localSourcePath: path,
                }
            }))
    }
}

impl IntoValue for BuildGraph {
    fn into_value(self, _: &mut Context) -> impl Value + use<> {
        self.nodes
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

fn get_vendor_dir<'a>(
    mut value: NixValue<'a>,
    ctx: &mut Context,
) -> Result<Cow<'a, Path>, NixError> {
    value.force_inline(ctx)?;

    match value.kind() {
        ValueKind::Attrset => NixDerivation::try_from_value(value, ctx)
            .and_then(|drv| drv.out_path(ctx))
            .map(Cow::Owned),

        ValueKind::Path => {
            <&'a Path>::try_from_value(value, ctx).map(Cow::Borrowed)
        },

        ValueKind::String => <String>::try_from_value(value, ctx)
            .map(|s| Cow::Owned(PathBuf::from(s))),

        _ => Err(NixError::new(
            ErrorKind::Nix,
            c"expected \"vendorDir\" to be a derivation, path, or string",
        )),
    }
}
