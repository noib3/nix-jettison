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
#[derive(Debug, derive_more::Display, cauchy::From)]
#[display("{_0}")]
pub(crate) enum VendorDepsError {
    /// A Nix runtime error occurred.
    Nix(#[from] NixError),

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

/// The functions that will have to be called to create the vendor directory.
struct CreateVendorDirFuns<'pkgs> {
    link_farm: NixFunction<'pkgs>,
    fetchurl: NixFunction<'pkgs>,
    fetch_git: NixFunction<'static>,
}

impl VendorDeps {
    fn create_vendor_dir<'a>(
        funs: CreateVendorDirFuns<'_>,
        deps: impl Iterator<Item = Dependency<'a>>,
        ctx: &mut Context,
    ) -> Result<Thunk<'static, NixAttrset<'static>>, NixError> {
        let mut links = Vec::new();

        for dep in deps {
            let source_path = match dep.source {
                DependencySource::CratesIo { checksum } => {
                    let url = format!(
                        "https://crates.io/api/v1/crates/{name}/{version}/download",
                        name = dep.name,
                        version = dep.version,
                    );
                    let args = attrset! {
                        url: url,
                        sha256: checksum,
                    };
                    funs.fetchurl.call::<NixAttrset>(args, ctx)?
                },
                DependencySource::Git { url, rev } => {
                    let args = attrset! {
                        url: url,
                        rev: rev,
                    };
                    funs.fetch_git.call::<NixAttrset>(args, ctx)?
                },
                DependencySource::Path => continue,
            };

            links.push(attrset! {
                name: dep.name,
                path: source_path,
            });
        }

        funs.link_farm.call_multi(
            (c"vendored-deps", links.into_list().into_value()),
            ctx,
        )
    }

    fn parse_lockfile(
        _cargo_lock: &str,
    ) -> Result<impl Iterator<Item = Dependency<'_>>, ParseCargoLockError>
    {
        Ok(core::iter::empty())
    }
}

impl<'pkgs> CreateVendorDirFuns<'pkgs> {
    fn new(
        pkgs: NixAttrset<'pkgs>,
        ctx: &mut Context,
    ) -> Result<Self, NixError> {
        Ok(Self {
            link_farm: pkgs.get(c"linkFarm", ctx)?,
            fetchurl: pkgs.get(c"fetchurl", ctx)?,
            fetch_git: todo!(),
        })
    }
}

impl Function for VendorDeps {
    type Args<'a> = VendorDepsArgs<'a>;

    fn call<'a: 'a>(
        args: Self::Args<'a>,
        ctx: &mut Context,
    ) -> Result<NixAttrset<'static>, VendorDepsError> {
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
            Ok(deps) => deps,
            Err(err) => {
                return Err(VendorDepsError::ParseCargoLock {
                    path: args.cargo_lock.into_owned(),
                    err,
                });
            },
        };

        let funs = CreateVendorDirFuns::new(args.pkgs, ctx)?;

        Self::create_vendor_dir(funs, deps, ctx)?
            .force(ctx)
            .map_err(Into::into)
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
