use core::ffi::CStr;
use core::result::Result;
use std::borrow::Cow;
use std::ffi::CString;
use std::path::Path;

use cargo::GlobalContext;
use cargo::core::compiler::{CompileKind, RustcTargetData};
use cargo::core::resolver::{CliFeatures, ForceAllTargets, HasDevUnits};
use cargo::core::{PackageIdSpec, Workspace};
use cargo::ops::{self, WorkspaceResolve};
use nix_bindings::prelude::{Error as NixError, *};

use crate::build_crate::BuildCrateArgs;

/// Resolves the build graph of a Rust package.
#[derive(nix_bindings::PrimOp)]
pub(crate) struct ResolveBuildGraph;

#[derive(nix_bindings::Args, nix_bindings::TryFromValue)]
#[args(flatten, name = "args")]
#[try_from(rename_all = "camelCase")]
pub(crate) struct ResolveBuildGraphArgs<'a> {
    /// The path to the root of the workspace the package is in.
    pub(crate) src: &'a Path,

    /// The package's name.
    pub(crate) package: String,

    /// The list of the package's features to enable.
    pub(crate) features: Vec<String>,

    /// The path to the directory where dependencies have been vendored.
    ///
    /// This can be obtained by calling `(jettison.vendorDeps { ... }).outPath`.
    pub(crate) vendor_dir: &'a Path,

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
    crates: Vec<BuildCrateArgs<'static, 'args, usize>>,
}

/// The type of error that can occur when resolving a build graph fails.
#[derive(Debug, derive_more::Display, cauchy::From)]
#[display("{_0}")]
pub(crate) enum ResolveBuildGraphError {
    /// Configuring the global Cargo context failed.
    ConfigureCargoContext(anyhow::Error),

    /// Creating the global Cargo context failed.
    CreateCargoContext(anyhow::Error),

    /// Constructing the [`RustcTargetData`] failed.
    CreateTargetData(anyhow::Error),

    /// Constructing the [`Workspace`] failed.
    CreateWorkspace(anyhow::Error),

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
        let mut ctx = GlobalContext::default()
            .map_err(ResolveBuildGraphError::CreateCargoContext)?;

        let vendored_sources = "vendored-sources";
        let vendor_dir = vendor_dir.display();

        let cli_config = vec![
            format!("source.crates-io.replace-with = '{vendored_sources}'"),
            format!("source.{vendored_sources}.directory = '{vendor_dir}'"),
        ];

        ctx.configure(
            0,
            false,
            None,
            true,
            true,
            true,
            &None,
            &[],
            &cli_config,
        )
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
        let _root_id = ws_resolve
            .targeted_resolve
            .iter()
            .find(|id| id.name().as_str() == args.package)
            .expect("root package not found in workspace resolve");

        todo!();
    }
}

impl Function for ResolveBuildGraph {
    type Args<'a> = ResolveBuildGraphArgs<'a>;

    fn call<'args>(
        args: Self::Args<'args>,
        _: &mut Context,
    ) -> Result<BuildGraph<'args>, ResolveBuildGraphError> {
        let manifest_path = args.src.join("Cargo.toml");

        let global_ctx = Self::cargo_ctx(args.vendor_dir)?;

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
            &[PackageIdSpec::new(args.package.clone())],
            HasDevUnits::No,
            ForceAllTargets::No,
            true,
        )
        .map_err(ResolveBuildGraphError::ResolveWorkspace)?;

        BuildGraph::new(&args, workspace_resolve)
    }
}

impl IntoValue for BuildGraph<'_> {
    fn into_value(self) -> impl Value {
        self.crates
    }
}

impl ToError for ResolveBuildGraphError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Nix
    }

    fn format_to_c_str(&self) -> Cow<'_, CStr> {
        CString::new(self.to_string())
            .expect("the Display impl doesn't contain any NUL bytes")
            .into()
    }
}
