use core::result::Result;
use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::{env, io};

use cargo::GlobalContext;
use cargo::core::compiler::{CompileKind, RustcTargetData};
use cargo::core::dependency::DepKind;
use cargo::core::resolver::{CliFeatures, ForceAllTargets, HasDevUnits};
use cargo::core::{PackageId, PackageIdSpec, Shell, Workspace};
use cargo::ops::{self, WorkspaceResolve};
use compact_str::CompactString;
use nix_bindings::prelude::{Error as NixError, *};

use crate::build_crate_args::{
    BuildCrateArgs,
    Dependencies,
    OptionalBuildCrateArgs,
    RequiredBuildCrateArgs,
};

/// Resolves the build graph of a Rust package.
#[derive(nix_bindings::PrimOp)]
pub(crate) struct ResolveBuildGraph;

#[derive(nix_bindings::Args, nix_bindings::TryFromValue)]
#[args(flatten, name = "args")]
#[try_from(rename_all = camelCase)]
pub(crate) struct ResolveBuildGraphArgs<'a> {
    /// The path to the root of the workspace the package is in.
    pub(crate) src: &'a Path,

    /// The package's name.
    pub(crate) package: CompactString,

    /// The path to the directory where dependencies have been vendored.
    ///
    /// This can be obtained by calling `jettison.vendorDeps { ... }`.
    #[try_from(with = get_vendor_dir)]
    pub(crate) vendor_dir: Cow<'a, Path>,

    /// The list of the package's features to enable.
    #[try_from(default)]
    pub(crate) features: Vec<String>,

    /// Whether to enable all features (equivalent to calling Cargo with the
    /// `--all-features` CLI flag).
    #[try_from(default)]
    pub(crate) all_features: bool,

    /// Whether to disable the default features (equivalent to calling Cargo
    /// with the `--no-default-features` CLI flag).
    #[try_from(default)]
    pub(crate) no_default_features: bool,
}

pub(crate) struct BuildGraph<'args> {
    pub(crate) crates: Vec<BuildCrateArgs<'args, usize>>,
    pkg_id_to_idx: HashMap<PackageId, usize>,
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

    /// A Nix runtime error occurred.
    Nix(#[from] NixError),

    /// Parsing the features failed.
    ParseFeatures(anyhow::Error),

    /// Resolving the [`Workspace`] failed.
    ResolveWorkspace(anyhow::Error),
}

impl ResolveBuildGraph {
    fn cargo_ctx(
        vendor_dir: &Path,
    ) -> Result<GlobalContext, ResolveBuildGraphError> {
        let shell = Shell::new();

        let cwd = env::current_dir().map_err(ResolveBuildGraphError::GetCwd)?;

        // The vendor directory created by `VendorDir::create()` contains a
        // `config.toml` file that configures Cargo to use the vendored
        // sources, so we can use it as the Cargo home.
        let cargo_home = vendor_dir;

        let mut ctx = GlobalContext::new(shell, cwd, cargo_home.to_owned());

        ctx.configure(0, false, None, true, true, true, &None, &[], &[])
            .map_err(ResolveBuildGraphError::ConfigureCargoContext)?;

        Ok(ctx)
    }
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

impl<'args> BuildGraph<'args> {
    fn new(
        args: &ResolveBuildGraphArgs<'args>,
        ws_resolve: WorkspaceResolve<'_>,
    ) -> Result<Self, ResolveBuildGraphError> {
        let root_id = ws_resolve
            .targeted_resolve
            .iter()
            .find(|id| id.name().as_str() == args.package)
            .expect("root package not found in workspace resolve");

        let mut this =
            Self { crates: Vec::new(), pkg_id_to_idx: HashMap::new() };

        Self::build_recursive(&mut this, root_id, args, &ws_resolve);

        Ok(this)
    }

    fn build_recursive(
        this: &mut Self,
        pkg_id: PackageId,
        args: &ResolveBuildGraphArgs<'args>,
        ws_resolve: &WorkspaceResolve<'_>,
    ) -> usize {
        let WorkspaceResolve { targeted_resolve, pkg_set, .. } = ws_resolve;

        // Fast path if we've already seen this package.
        if let Some(&idx) = this.pkg_id_to_idx.get(&pkg_id) {
            return idx;
        }

        let mut dependencies = Dependencies::default();

        for (dep_id, dep_set) in targeted_resolve.deps(pkg_id) {
            let dep_idx = Self::build_recursive(this, dep_id, args, ws_resolve);

            for dep in dep_set {
                match dep.kind() {
                    DepKind::Normal => dependencies.normal.push(dep_idx),
                    DepKind::Development => {},
                    DepKind::Build => dependencies.build.push(dep_idx),
                }
            }
        }

        let package =
            pkg_set.get_one(pkg_id).expect("package ID not found in workspace");

        let build_crate_args = BuildCrateArgs {
            required: RequiredBuildCrateArgs::new(package, args),
            optional: OptionalBuildCrateArgs::new(package, targeted_resolve),
            dependencies,
        };

        let idx = this.crates.len();

        this.crates.push(build_crate_args);
        this.pkg_id_to_idx.insert(pkg_id, idx);

        idx
    }
}

impl Function for ResolveBuildGraph {
    type Args<'a> = ResolveBuildGraphArgs<'a>;

    fn call<'args>(
        args: Self::Args<'args>,
        _: &mut Context,
    ) -> Result<BuildGraph<'args>, ResolveBuildGraphError> {
        let manifest_path = args.src.join("Cargo.toml");

        let global_ctx = Self::cargo_ctx(&args.vendor_dir)?;

        let workspace = Workspace::new(&manifest_path, &global_ctx)
            .map_err(ResolveBuildGraphError::CreateWorkspace)?;

        let target = args.compile_target()?;

        let mut target_data = RustcTargetData::new(&workspace, &[target])
            .map_err(ResolveBuildGraphError::CreateTargetData)?;

        let workspace_resolve = ops::resolve_ws_with_opts(
            &workspace,
            &mut target_data,
            &[target],
            &args.features()?,
            &[PackageIdSpec::new(args.package.clone().into())],
            HasDevUnits::No,
            ForceAllTargets::No,
            true,
        )
        .map_err(ResolveBuildGraphError::ResolveWorkspace)?;

        BuildGraph::new(&args, workspace_resolve)
    }
}

impl<'a> IntoValue for BuildGraph<'a> {
    fn into_value(self, _: &mut Context) -> impl Value + use<'a> {
        self.crates
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
