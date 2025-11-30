use core::ffi::CStr;
use core::result::Result;
use std::borrow::Cow;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::{fs, io};

use either::Either;
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
    CratesIo(CratesIoSource<'lock>),

    /// TODO: docs.
    Git(GitSource<'lock>),

    /// TODO: docs.
    Path,
}

/// TODO: docs.
#[derive(Debug, Copy, Clone)]
pub(crate) struct CratesIoSource<'lock> {
    checksum: &'lock str,
}

/// TODO: docs.
#[derive(Debug, Copy, Clone)]
pub(crate) struct GitSource<'lock> {
    url: &'lock str,
    rev: &'lock str,
    url_fragment: Option<&'lock str>,
    branch: Option<&'lock str>,
    tag: Option<&'lock str>,
}

/// The functions that will have to be called to create the vendor directory.
struct CreateVendorDirFuns<'pkgs> {
    link_farm: NixLambda<'pkgs>,
    fetchurl: NixFunctor<'pkgs>,
    fetch_git: NixLambda<'static>,
}

impl VendorDeps {
    fn create_vendor_dir<'a>(
        funs: CreateVendorDirFuns<'_>,
        deps: impl Iterator<Item = Dependency<'a>>,
        ctx: &mut Context,
    ) -> Result<Thunk<'static, NixAttrset<'static>>, NixError> {
        let mut links = Vec::new();

        for dep in deps {
            let Dependency { name, version, .. } = dep;

            let source_path = match dep.source {
                DependencySource::CratesIo(src) => {
                    let args = attrset! {
                        name: format!("{name}-{version}.tar.gz"),
                        url: format!("https://static.crates.io/crates/{name}/{name}-{version}.crate"),
                        sha256: src.checksum,
                    };
                    funs.fetchurl.call::<NixAttrset>(args, ctx)?
                },
                DependencySource::Git(src) => {
                    let r#ref = src.branch.map(Cow::Borrowed).or_else(|| {
                        src.tag.map(|tag| format!("refs/tags/{tag}").into())
                    });

                    let args = attrset! {
                        url: src.url,
                        rev: src.url_fragment.unwrap_or(src.rev),
                        submodules: true,
                    }
                    .merge(match r#ref {
                        Some(r#ref) => Either::Left(attrset! { ref: r#ref, }),
                        None => Either::Right(attrset! { allRefs: true, }),
                    });

                    funs.fetch_git.call::<NixAttrset>(args, ctx)?
                },
                DependencySource::Path => continue,
            };

            links.push(attrset! {
                name: format!("{name}-{version}"),
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
        Ok([
            Dependency {
                name: "abs-path",
                version: "0.1.0",
                source: DependencySource::Git(GitSource {
                    url: "https://github.com/nomad/abs-path.git",
                    rev: "c9f47071a05cc80bcff4af7b65754a23f2edb6ad",
                    url_fragment: None,
                    branch: None,
                    tag: None,
                }),
            },
            Dependency {
                name: "anstream",
                version: "0.6.21",
                source: DependencySource::CratesIo(CratesIoSource {
                    checksum: "43d5b281e737544384e969a5ccad3f1cdd24b48086a0fc1b2a5262a26b8f4f4a",
                }),
            },
            Dependency {
                name: "local-dep",
                version: "0.1.0",
                source: DependencySource::Path,
            },
        ]
        .into_iter())
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
            fetch_git: ctx.builtins().fetch_git(ctx),
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
