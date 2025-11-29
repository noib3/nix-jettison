use core::ffi::CStr;
use core::result::Result;
use std::borrow::Cow;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::{fs, io};

use nix_bindings::prelude::{Error as NixError, *};

/// Vendors the dependencies of a Rust package.
#[derive(nix_bindings::PrimOp)]
pub(crate) struct VendorDeps;

#[derive(nix_bindings::Args, nix_bindings::TryFromValue)]
#[args(flatten, name = "args")]
pub(crate) struct VendorDepsArgs<'a> {
    pub(crate) pkgs: NixAttrset<'a>,
    pub(crate) cargo_lock: Cow<'a, Path>,
}

/// The type of error that can occur when vendoring dependencies fails.
#[derive(Debug, derive_more::Display)]
#[display("{_0}")]
pub(crate) enum VendorDepsError {
    /// The `fetchGit` function was not found in the provided `pkgs`.
    MissingFetchGit(NixError),

    /// The `fetchurl` function was not found in the provided `pkgs`.
    MissingFetchurl(NixError),

    /// Parsing the contents of the `Cargo.lock` failed.
    #[display("failed to parse Cargo.lock at {path:?}: {err}")]
    ParseCargoLock { path: PathBuf, err: ParseCargoLockError },

    /// Reading the `Cargo.lock` into a string failed.
    #[display("failed to read Cargo.lock at {path:?}: {err}")]
    ReadCargoLock { path: PathBuf, err: io::Error },
}

/// The type of error that can occur when parsing the contents of a
/// `Cargo.lock` file fails.
#[derive(Debug, derive_more::Display)]
#[display("{_0}")]
pub(crate) enum ParseCargoLockError {}

/// TODO: docs.
#[derive(Debug, Copy, Clone)]
pub(crate) struct Dependency<'lock> {
    name: &'lock str,
    version: &'lock str,
    source: DependencySource<'lock>,
}

/// TODO: docs.
#[derive(Debug, Copy, Clone)]
pub(crate) enum DependencySource<'lock> {
    /// TODO: docs.
    CratesIo { checksum: &'lock str },

    /// TODO: docs.
    Git { url: &'lock str, rev: &'lock str },

    /// TODO: docs.
    Path,
}

impl VendorDeps {
    fn create_vendor_dir(
        _deps: &[Dependency<'_>],
        _fetchurl: NixValue<'_>,
        _fetch_git: NixValue<'_>,
    ) -> impl Value + use<> {
    }

    fn parse_lockfile(
        _cargo_lock: &str,
    ) -> Result<impl Iterator<Item = Dependency<'_>>, ParseCargoLockError>
    {
        Ok(core::iter::empty())
    }
}

impl Function for VendorDeps {
    type Args<'a> = VendorDepsArgs<'a>;

    fn call<'a: 'a>(
        args: Self::Args<'a>,
        ctx: &mut Context,
    ) -> Result<impl Value + use<>, VendorDepsError> {
        let cargo_lock = match fs::read_to_string(&args.cargo_lock) {
            Ok(contents) => contents,
            Err(err) => {
                return Err(VendorDepsError::ReadCargoLock {
                    path: args.cargo_lock.into_owned(),
                    err,
                });
            },
        };

        let deps = match Self::parse_lockfile(&cargo_lock) {
            Ok(deps) => deps.collect::<Vec<_>>(),
            Err(err) => {
                return Err(VendorDepsError::ParseCargoLock {
                    path: args.cargo_lock.into_owned(),
                    err,
                });
            },
        };

        let fetchurl = args
            .pkgs
            .get(c"fetchurl", ctx)
            .map_err(VendorDepsError::MissingFetchurl)?;

        let fetch_git = args
            .pkgs
            .get(c"fetchGit", ctx)
            .map_err(VendorDepsError::MissingFetchGit)?;

        Ok(Self::create_vendor_dir(&deps, fetchurl, fetch_git))
    }
}

impl ToError for VendorDepsError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Nix
    }

    fn format_to_c_str(&self) -> Cow<'_, CStr> {
        CString::new(self.to_string())
            .expect("the Display impl doesn't contain any NUL bytes")
            .into()
    }
}
