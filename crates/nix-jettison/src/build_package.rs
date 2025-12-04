use core::ffi::CStr;
use core::result::Result;
use std::borrow::Cow;
use std::env;
use std::ffi::CString;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use cargo::core::compiler::{CompileKind, RustcTargetData};
use cargo::core::resolver::{CliFeatures, ForceAllTargets, HasDevUnits};
use cargo::core::{PackageIdSpec, Shell, Workspace};
use cargo::{GlobalContext, ops};
use nix_bindings::prelude::{Error as NixError, *};

use crate::vendor_deps::{VendorDeps, VendorDepsArgs, VendorDepsError};

/// Builds a Rust package.
#[derive(nix_bindings::PrimOp)]
pub(crate) struct BuildPackage;

#[derive(nix_bindings::Args, nix_bindings::TryFromValue)]
#[args(flatten, name = "args")]
#[try_from(rename_all = "camelCase")]
pub(crate) struct BuildPackageArgs<'a> {
    pkgs: NixAttrset<'a>,
    src: &'a Path,
    package: String,
    all_features: bool,
    no_default_features: bool,
}

/// The type of error that can occur when building a package fails.
#[derive(Debug, derive_more::Display, cauchy::From)]
#[display("{_0}")]
pub(crate) enum BuildPackageError {
    /// Configuring the global Cargo context failed.
    ConfigureCargoContext(anyhow::Error),

    /// Constructing the [`RustcTargetData`] failed.
    CreateTargetData(anyhow::Error),

    /// Constructing the [`Workspace`] failed.
    CreateWorkspace(anyhow::Error),

    /// Getting the current working directory failed..
    Cwd(anyhow::Error),

    /// A Nix runtime error occurred.
    Nix(#[from] NixError),

    /// Parsing the features failed.
    ParseFeatures(anyhow::Error),

    /// Resolving the [`Workspace`] failed.
    ResolveWorkspace(anyhow::Error),

    /// Vendoring the dependencies failed.
    VendorDeps(#[from] VendorDepsError),
}

impl BuildPackage {
    fn cargo_ctx(
        cargo_home: PathBuf,
    ) -> Result<GlobalContext, BuildPackageError> {
        let cwd = env::current_dir()
            .context("couldn't get the current directory of the process")
            .map_err(BuildPackageError::Cwd)?;

        let mut ctx = GlobalContext::new(Shell::new(), cwd, cargo_home);

        ctx.configure(0, false, None, true, true, true, &None, &[], &[])
            .map_err(BuildPackageError::ConfigureCargoContext)?;

        Ok(ctx)
    }

    fn generate_cargo_config(vendor_dir: &Path) -> String {
        let vendored_sources = "vendored-sources";

        format!(
            r#"
[source.crates-io]
replace-with = "{vendored_sources}"

[source.{vendored_sources}]
directory = "{}"
"#,
            vendor_dir.display()
        )
    }
}

impl BuildPackageArgs<'_> {
    fn compile_target(
        &self,
        _ctx: &mut Context,
    ) -> Result<CompileKind, BuildPackageError> {
        Ok(CompileKind::Host)
    }

    fn features(
        &self,
        _ctx: &mut Context,
    ) -> Result<CliFeatures, BuildPackageError> {
        CliFeatures::from_command_line(
            &[],
            self.all_features,
            !self.no_default_features,
        )
        .map_err(BuildPackageError::ParseFeatures)
    }
}

impl Function for BuildPackage {
    type Args<'a> = BuildPackageArgs<'a>;

    fn call<'a: 'a>(
        args: Self::Args<'a>,
        ctx: &mut Context,
    ) -> Result<impl Value + use<>, BuildPackageError> {
        let vendor_args = VendorDepsArgs {
            pkgs: args.pkgs,
            cargo_lock: args.src.join("Cargo.lock").into(),
        };

        let vendor_dir = <VendorDeps as Function>::call(vendor_args, ctx)?;

        let cargo_config =
            Self::generate_cargo_config(&vendor_dir.out_path(ctx)?);

        let cargo_home = args
            .pkgs
            .get::<NixLambda>(c"writeTextDir", ctx)?
            .call_multi::<NixDerivation>(("config.toml", cargo_config), ctx)?
            .force(ctx)?;

        let global_ctx = Self::cargo_ctx(cargo_home.out_path(ctx)?)?;

        let manifest_path = args.src.join("Cargo.toml");

        let workspace = Workspace::new(&manifest_path, &global_ctx)
            .map_err(BuildPackageError::CreateWorkspace)?;

        let target = args.compile_target(ctx)?;

        let mut target_data = RustcTargetData::new(&workspace, &[target])
            .map_err(BuildPackageError::CreateTargetData)?;

        let _workspace_resolve = ops::resolve_ws_with_opts(
            &workspace,
            &mut target_data,
            &[target],
            &args.features(ctx)?,
            &[PackageIdSpec::new(args.package.to_owned())],
            HasDevUnits::No,
            ForceAllTargets::No,
            true,
        )
        .map_err(BuildPackageError::ResolveWorkspace)?;

        Ok(workspace
            .members()
            .map(|pkg| pkg.name().as_str())
            .collect::<Vec<_>>()
            .into_list()
            .into_value())
    }
}

impl ToError for BuildPackageError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Nix
    }

    fn format_to_c_str(&self) -> Cow<'_, CStr> {
        CString::new(self.to_string())
            .expect("the Display impl doesn't contain any NUL bytes")
            .into()
    }
}
