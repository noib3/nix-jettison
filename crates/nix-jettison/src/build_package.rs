use core::ffi::CStr;
use core::result::Result;
use std::borrow::Cow;
use std::collections::HashMap;
use std::env;
use std::ffi::CString;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use cargo::core::compiler::{CompileKind, RustcTargetData};
use cargo::core::resolver::{CliFeatures, ForceAllTargets, HasDevUnits};
use cargo::core::{
    Package,
    PackageId,
    PackageIdSpec,
    Resolve,
    Shell,
    Workspace,
};
use cargo::ops::WorkspaceResolve;
use cargo::{GlobalContext, ops};
use nix_bindings::prelude::{Error as NixError, *};

use crate::vendor_deps::{
    VendorDeps,
    VendorDepsArgs,
    VendorDepsError,
    VendorDir,
};

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

struct BuildGraph {
    crates: HashMap<PackageId, Thunk<'static, NixDerivation<'static>>>,
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
            &self.features,
            self.all_features,
            !self.no_default_features,
        )
        .map_err(BuildPackageError::ParseFeatures)
    }
}

impl BuildGraph {
    fn new(
        root_pkg_name: &str,
        ws_resolve: WorkspaceResolve<'_>,
        _ctx: &mut Context,
    ) -> Result<Self, NixError> {
        let _root_id = ws_resolve
            .targeted_resolve
            .iter()
            .find(|id| id.name().as_str() == root_pkg_name)
            .expect("root package not found in workspace resolve");

        todo!();
    }

    #[allow(clippy::too_many_arguments)]
    fn build_rust_crate(
        resolve: &Resolve,
        package: &Package,
        deps: &[Thunk<'static, NixDerivation<'static>>],
        build_deps: &[Thunk<'static, NixDerivation<'static>>],
        vendor_dir: &VendorDir,
        pkgs: NixAttrset,
        ctx: &mut Context,
    ) -> Result<Thunk<'static, NixDerivation<'static>>, NixError> {
        let pkg_id = package.package_id();

        let src =
            if pkg_id.source_id().is_path() {
                Cow::Borrowed(package.root())
            } else {
                Cow::Owned(vendor_dir.get_package_src(
                    package.name().as_str(),
                    package.version(),
                ))
            };

        let features = resolve
            .features(pkg_id)
            .iter()
            .map(|feature| feature.as_str())
            .into_value();

        let custom_lib_name = package.targets().iter().find_map(|target| {
            if !target.is_lib() {
                None
            } else {
                (pkg_id.name() != target.name()).then(|| target.name())
            }
        });

        // See https://github.com/NixOS/nixpkgs/blob/d792a6e0cd4ba35c90ea787b717d72410f56dc40/pkgs/build-support/rust/build-rust-crate/default.nix#L232-L251
        // for the list of arguments processed by `buildRustCrate`.
        let args = attrset! {
            src: src,
            release: true,
            crateName: pkg_id.name().as_str(),
            version: pkg_id.version().to_string(),
            dependencies: deps.into_value(),
            features: features,
            edition: package.manifest().edition().to_string(),
            procMacro: package.proc_macro(),
        }
        .merge(custom_lib_name.map(|name| attrset! { libName: name }))
        .merge((!build_deps.is_empty()).then(|| {
            attrset! {
                buildDependencies: build_deps.into_value(),
            }
        }));

        pkgs.get::<NixFunctor>(c"buildRustCrate", ctx)?.call(args, ctx)
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

        let cargo_config = Self::generate_cargo_config(vendor_dir.path());

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
            &[PackageIdSpec::new(args.package.clone())],
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
