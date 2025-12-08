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
    GitSource,
    GitSourceRef,
    PackageEntry,
    PackageSource,
    RegistryKind,
    RegistrySource,
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
    extract_crate: NixLambda<'static>,
    fetch_git: NixLambda<'builtins>,
    fetchurl: NixFunctor<'pkgs>,
    link_farm: NixLambda<'pkgs>,
    run_command_local: NixLambda<'pkgs>,
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
            let Some(source) = source else { continue };
            let download_drv = source.download(name, version, &funs, ctx)?;
            links.push(attrset! {
                name: Self::dir_name(name, version),
                path: download_drv.into_value(),
            });
        }

        let derivation = funs
            .link_farm
            .call_multi((c"vendored-deps", links), ctx)?
            .force_into::<NixDerivation>(ctx)?;

        let out_path = derivation.out_path(ctx)?;

        Ok(Self { out_path, derivation })
    }
}

impl<'lock> PackageSource<'lock> {
    #[allow(clippy::too_many_arguments)]
    fn download<'pkgs>(
        &self,
        pkg_name: &'lock str,
        pkg_version: &'lock str,
        funs: &CreateVendorDirFuns<'pkgs, '_>,
        ctx: &mut Context,
    ) -> Result<impl Value + use<'lock, 'pkgs>, NixError> {
        Ok(match self {
            Self::Registry(src) => {
                Either::Left(src.download(pkg_name, pkg_version, funs, ctx)?)
            },
            Self::Git(src) => Either::Right(src.download(funs.fetch_git, ctx)?),
        })
    }
}

impl<'lock> RegistrySource<'lock> {
    #[allow(clippy::too_many_arguments)]
    fn download<'pkgs>(
        &self,
        pkg_name: &'lock str,
        pkg_version: &'lock str,
        funs: &CreateVendorDirFuns<'pkgs, '_>,
        ctx: &mut Context,
    ) -> Result<impl Value + use<'lock, 'pkgs>, NixError> {
        match self.kind {
            RegistryKind::CratesIo => {},
            RegistryKind::Other { .. } => {
                panic!("custom registries are not yet supported")
            },
        }

        let fetchurl_args = attrset! {
            name: format!("{pkg_name}-{pkg_version}.tar.gz"),
            url: format!("https://static.crates.io/crates/{pkg_name}/{pkg_name}-{pkg_version}.crate"),
            sha256: self.checksum,
        };

        let drv = funs.fetchurl.call(fetchurl_args, ctx)?;

        let extract_crate_args = attrset! {
            drv: drv,
            checksum: self.checksum,
            name: format!("{pkg_name}-{pkg_version}"),
            runCommandLocal: funs.run_command_local,
        };

        funs.extract_crate.call(extract_crate_args, ctx)
    }
}

impl GitSource<'_> {
    fn download(
        &self,
        fetch_git: NixLambda,
        ctx: &mut Context,
    ) -> Result<Thunk<'static>, NixError> {
        let r#ref = self.r#ref.and_then(GitSourceRef::format_for_fetch_git);

        let args = attrset! {
            url: self.url,
            rev: self.rev,
            submodules: true,
            postFetch: "echo '{\"package\":null,\"files\":{}}' > $out/.cargo-checksum.json",
        }
        .merge(match r#ref {
            Some(r#ref) => Either::Left(attrset! { ref: r#ref }),
            None => Either::Right(attrset! { allRefs: true }),
        });

        fetch_git.call(args, ctx)
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

        let extract_crate = ctx.eval(c"
            { drv, checksum, name, runCommandLocal }:
            runCommandLocal name {} ''
              mkdir -p $out
              tar -xzf ${drv} --strip-components 1 -C $out
              echo '{\"package\":\"${checksum}\",\"files\":{}}' > $out/.cargo-checksum.json
            ''
        ")?;

        let funs = CreateVendorDirFuns {
            extract_crate,
            link_farm: args.pkgs.get(c"linkFarm", ctx)?,
            fetchurl: args.pkgs.get(c"fetchurl", ctx)?,
            fetch_git: ctx.builtins().fetch_git(ctx),
            run_command_local: args.pkgs.get(c"runCommandLocal", ctx)?,
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
