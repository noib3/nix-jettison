use core::ffi::CStr;
use core::result::Result;
use std::borrow::Cow;
use std::env;
use std::ffi::CString;
use std::path::Path;

use anyhow::Context as _;
use cargo::GlobalContext;
use cargo::core::{Shell, Workspace};
use nix_bindings::prelude::{Error as NixError, *};

use crate::vendor_deps::{VendorDeps, VendorDepsArgs, VendorDepsError};

/// Builds a Rust package.
#[derive(nix_bindings::PrimOp)]
pub(crate) struct BuildPackage;

#[derive(nix_bindings::Args, nix_bindings::TryFromValue)]
#[args(flatten, name = "args")]
pub(crate) struct BuildPackageArgs<'a> {
    pkgs: NixAttrset<'a>,
    src: &'a Path,
}

/// The type of error that can occur when building a package fails.
#[derive(Debug, derive_more::Display, cauchy::From)]
#[display("{_0}")]
pub(crate) enum BuildPackageError {
    /// Constructing the [`Workspace`] failed.
    CreateWorkspace(anyhow::Error),

    /// Getting the current working directory failed..
    Cwd(anyhow::Error),

    /// A Nix runtime error occurred.
    Nix(#[from] NixError),

    /// Vendoring the dependencies failed.
    VendorDeps(#[from] VendorDepsError),
}

impl BuildPackage {
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

        let vendor_dir_drv = <VendorDeps as Function>::call(vendor_args, ctx)?;
        let vendor_dir = vendor_dir_drv.get::<String>(c"outPath", ctx)?;
        let cargo_config = Self::generate_cargo_config(Path::new(&vendor_dir));

        let cargo_home_drv = args
            .pkgs
            .get::<NixLambda>(c"writeTextDir", ctx)?
            .call_multi::<NixAttrset>(("config.toml", cargo_config), ctx)?
            .force(ctx)?;

        let cargo_home_path =
            cargo_home_drv.get::<&Path>(c"outPath", ctx)?.to_owned();

        let cwd = env::current_dir()
            .context("couldn't get the current directory of the process")
            .map_err(BuildPackageError::Cwd)?;

        let global_ctx =
            GlobalContext::new(Shell::new(), cwd, cargo_home_path);

        let manifest_path = args.src.join("Cargo.toml");

        let workspace = Workspace::new(&manifest_path, &global_ctx)
            .map_err(BuildPackageError::CreateWorkspace)?;

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
