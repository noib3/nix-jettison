use core::ffi::CStr;
use core::result::Result;
use std::borrow::Cow;
use std::ffi::CString;
use std::path::Path;

use cargo::GlobalContext;
use cargo::core::Workspace;
use nix_bindings::prelude::*;

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
#[derive(Debug, derive_more::Display)]
#[display("{_0}")]
pub(crate) enum BuildPackageError {
    /// Constructing the [`GlobalContext`] failed.
    CreateContext(anyhow::Error),

    /// Constructing the [`Workspace`] failed.
    CreateWorkspace(anyhow::Error),
}

impl Function for BuildPackage {
    type Args<'a> = BuildPackageArgs<'a>;

    fn call<'a: 'a>(
        args: Self::Args<'a>,
        _: &mut Context,
    ) -> Result<impl Value + use<>, BuildPackageError> {
        let manifest_path = args.src.join("Cargo.toml");

        let global_ctx = GlobalContext::default()
            .map_err(BuildPackageError::CreateContext)?;

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
