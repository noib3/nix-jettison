use core::ffi::CStr;
use core::fmt::Display;
use core::result::Result;
use std::borrow::Cow;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::{fs, io};

use compact_str::{CompactString, format_compact};
use either::Either;
use nix_bindings::prelude::{Error as NixError, *};

use crate::cargo_lock_parser::{
    CargoLockParseError,
    CargoLockParser,
    PackageEntry,
    PackageSource,
    RegistryKind,
};

/// Vendors the dependencies of a Rust package.
#[derive(nix_bindings::PrimOp)]
pub(crate) struct VendorDeps;

#[derive(nix_bindings::Args, nix_bindings::TryFromValue)]
#[args(flatten, name = "args")]
#[try_from(rename_all = camelCase)]
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
    ParseCargoLock { path: PathBuf, err: CargoLockParseError },

    /// Reading the `Cargo.lock` into a string failed.
    #[display("failed to read Cargo.lock at {path:?}: {err}")]
    ReadCargoLock { path: PathBuf, err: io::Error },
}

/// TODO: docs.
pub(crate) struct VendorDir {
    derivation: NixDerivation<'static>,
    out_path: PathBuf,
}

/// The functions that will have to be called to create the vendor directory.
struct CreateVendorDirFuns<'pkgs, 'builtins> {
    link_farm: NixLambda<'pkgs>,
    fetchurl: NixFunctor<'pkgs>,
    fetch_git: NixLambda<'builtins>,
}

impl VendorDir {
    pub(crate) fn dir_name(
        pkg_name: &str,
        pkg_version: impl Display,
    ) -> CompactString {
        format_compact!("{pkg_name}-{pkg_version}")
    }

    pub(crate) fn path(&self) -> &Path {
        &self.out_path
    }

    fn create<'a, Err>(
        deps: impl Iterator<Item = Result<PackageEntry<'a>, Err>>,
        funs: CreateVendorDirFuns,
        ctx: &mut Context,
    ) -> Result<Self, VendorDepsError>
    where
        VendorDepsError: From<Err>,
    {
        let mut links = Vec::new();

        for dep_res in deps {
            let PackageEntry { name, version, source } = dep_res?;

            let source_path = match source {
                PackageSource::Registry(src) => {
                    match src.kind {
                        RegistryKind::CratesIo => {},
                        RegistryKind::Other { .. } => {
                            panic!("custom registries are not yet supported")
                        },
                    }

                    let args = attrset! {
                        name: format!("{name}-{version}.tar.gz"),
                        url: format!("https://static.crates.io/crates/{name}/{name}-{version}.crate"),
                        sha256: src.checksum,
                    };
                    funs.fetchurl.call::<NixAttrset>(args, ctx)?
                },
                PackageSource::Git(src) => {
                    let r#ref = src.branch.map(Cow::Borrowed).or_else(|| {
                        src.tag.map(|tag| format!("refs/tags/{tag}").into())
                    });

                    let args = attrset! {
                        url: src.url,
                        rev: src.url_fragment.unwrap_or(src.rev),
                        submodules: true,
                    }
                    .merge(match r#ref {
                        Some(r#ref) => Either::Left(attrset! { ref: r#ref }),
                        None => Either::Right(attrset! { allRefs: true }),
                    });

                    funs.fetch_git.call::<NixAttrset>(args, ctx)?
                },
                PackageSource::Path => continue,
            };

            links.push(attrset! {
                name: Self::dir_name(name, version),
                path: source_path,
            });
        }

        let derivation = funs
            .link_farm
            .call_multi::<NixDerivation>((c"vendored-deps", links), ctx)?
            .force(ctx)?;

        let out_path = derivation.out_path(ctx)?;

        Ok(Self { out_path, derivation })
    }
}

impl Function for VendorDeps {
    type Args<'a> = VendorDepsArgs<'a>;

    fn call<'a: 'a>(
        args: Self::Args<'a>,
        ctx: &mut Context,
    ) -> Result<VendorDir, VendorDepsError> {
        let cargo_lock = match fs::read_to_string(&args.cargo_lock) {
            Ok(contents) => contents,
            Err(err) => {
                return Err(VendorDepsError::ReadCargoLock {
                    path: args.cargo_lock.into_owned(),
                    err,
                });
            },
        };

        let deps = CargoLockParser::new(&cargo_lock).map(|res| {
            res.map_err(|err| VendorDepsError::ParseCargoLock {
                path: (*args.cargo_lock).to_owned(),
                err,
            })
        });

        let funs = CreateVendorDirFuns {
            link_farm: args.pkgs.get(c"linkFarm", ctx)?,
            fetchurl: args.pkgs.get(c"fetchurl", ctx)?,
            fetch_git: ctx.builtins().fetch_git(ctx),
        };

        VendorDir::create(deps, funs, ctx)
    }
}

impl IntoValue for VendorDir {
    fn into_value(self) -> impl Value {
        self.derivation
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
