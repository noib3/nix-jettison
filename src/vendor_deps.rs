use core::cell::OnceCell;
use core::cmp::Ordering;
use core::fmt::{self, Write};
use core::iter;
use core::result::Result;
use std::borrow::Cow;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::{fs, io};

use compact_str::{CompactString, ToCompactString};
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
pub(crate) enum VendorDepsError {
    /// A Nix runtime error occurred.
    #[display("{_0}")]
    Nix(#[from] NixError),

    /// Parsing the contents of the `Cargo.lock` failed.
    #[display("failed to parse Cargo.lock: {_0}")]
    ParseCargoLock(#[from] CargoLockParseError),

    /// Reading the `Cargo.lock` into a string failed.
    #[display("failed to read Cargo.lock at {path:?}: {err}")]
    ReadCargoLock { path: PathBuf, err: io::Error },
}

/// TODO: docs.
pub(crate) struct VendoredSources<'lock> {
    sources: Vec<Source<'lock>>,
    config_dot_toml: String,
}

#[derive(Clone, Debug)]
pub(crate) struct SourceId<'a> {
    pub(crate) package_name: &'a str,
    pub(crate) version: Cow<'a, str>,
}

struct Source<'lock> {
    id: SourceId<'lock>,
    derivation: Thunk<'static>,
}

impl VendorDeps {
    pub(crate) fn read_cargo_lock(
        cargo_lock_path: &Path,
    ) -> Result<String, VendorDepsError> {
        fs::read_to_string(cargo_lock_path).map_err(|err| {
            VendorDepsError::ReadCargoLock {
                path: cargo_lock_path.to_owned(),
                err,
            }
        })
    }
}

impl<'lock> VendoredSources<'lock> {
    pub(crate) fn get(&self, source_id: SourceId) -> Option<Thunk<'static>> {
        self.sources
            .binary_search_by(|probe| probe.id.cmp(&source_id))
            .ok()
            .map(|idx| self.sources[idx].derivation)
    }

    pub(crate) fn new(
        cargo_lock: &'lock str,
        pkgs: NixAttrset,
        ctx: &mut Context,
    ) -> Result<Self, VendorDepsError> {
        let replace_with = "vendored-sources";

        let mut sources = Vec::new();

        let mut config_dot_toml = format!(
            r#"[source.crates-io]
replace-with = "{replace_with}"

[source.{replace_with}]
directory = "."
"#
        );

        let fetch_git = ctx.builtins().fetch_git(ctx);
        let fetchurl = pkgs.get(c"fetchurl", ctx)?;
        let run_command_local = pkgs.get(c"runCommandLocal", ctx)?;

        for res in CargoLockParser::new(cargo_lock) {
            let PackageEntry { name, version, source } = res?;

            let Some(source) = source else { continue };

            let derivation = match source {
                PackageSource::Registry(source) => source.fetch(
                    name,
                    version,
                    fetchurl,
                    run_command_local,
                    ctx,
                )?,
                PackageSource::Git(source) => {
                    write!(
                        &mut config_dot_toml,
                        "{}",
                        source.into_cargo_config_entry(replace_with)
                    )
                    .expect("writing to a String cannot fail");
                    source.fetch(fetch_git, run_command_local, ctx)?
                },
            };

            let source_id = SourceId {
                package_name: name,
                version: Cow::Borrowed(version),
            };

            // Make sure the entries returned by the iterator are already
            // sorted by source ID, so that we can just push to the vector.
            debug_assert!(
                sources.last().is_none_or(|last: &Source| last.id < source_id)
            );

            sources.push(Source { id: source_id, derivation });
        }

        Ok(Self { sources, config_dot_toml })
    }

    pub(crate) fn to_dir(
        &self,
        pkgs: NixAttrset,
        ctx: &mut Context,
    ) -> Result<NixDerivation<'static>, NixError> {
        let write_text_file = pkgs.get::<NixLambda>(c"writeTextFile", ctx)?;

        let config_dot_toml_drv = write_text_file.call(
            attrset! {
                name: "config.toml",
                text: &*self.config_dot_toml,
            },
            ctx,
        )?;

        let entries = self
            .sources
            .iter()
            .map(|source| {
                attrset! {
                    name: source.id.to_compact_string(),
                    path: source.derivation,
                }
            })
            .chain(iter::once(attrset! {
                name: CompactString::const_new(".cargo/config.toml"),
                path: config_dot_toml_drv,
            }))
            // TODO: is `Chain` not `ExactSize`?
            .collect::<Vec<_>>();

        pkgs.get::<NixLambda>(c"linkFarm", ctx)?
            .call_multi((c"vendored-sources", entries), ctx)?
            .force_into(ctx)
    }
}

impl<'lock> RegistrySource<'lock> {
    #[expect(clippy::too_many_arguments)]
    fn fetch(
        &self,
        pkg_name: &str,
        pkg_version: &str,
        fetchurl: NixFunctor,
        run_command_local: NixLambda,
        ctx: &mut Context,
    ) -> Result<Thunk<'static>, NixError> {
        thread_local! {
            static WRAP: OnceCell<NixLambda<'static>> = const { OnceCell::new() };
        }

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

        let extract_and_add_checksum_args = attrset! {
            src: fetchurl.call(fetchurl_args, ctx)?,
            checksum: self.checksum,
            name: format!("{pkg_name}-{pkg_version}"),
            runCommandLocal: run_command_local,
        };

        let extract_and_add_checksum = WRAP.with(|cell| match cell.get().copied() {
            Some(wrap) => Ok::<_, NixError>(wrap),
            None => {
                let wrap = ctx.eval::<NixLambda>(c"
                    { src, checksum, name, runCommandLocal }:
                    runCommandLocal name {} ''
                      mkdir -p $out
                      tar -xzf ${src} --strip-components 1 -C $out
                      echo '{\"package\":\"${checksum}\",\"files\":{}}' > $out/.cargo-checksum.json
                    ''
                ")?;
                Ok(*cell.get_or_init(|| wrap))
            },
        })?;

        extract_and_add_checksum.call(extract_and_add_checksum_args, ctx)
    }
}

impl GitSource<'_> {
    fn fetch(
        &self,
        fetch_git: NixLambda,
        run_command_local: NixLambda,
        ctx: &mut Context,
    ) -> Result<Thunk<'static>, NixError> {
        thread_local! {
            static WRAP: OnceCell<NixLambda<'static>> = const { OnceCell::new() };
        }

        let r#ref = self.r#ref.and_then(GitSourceRef::format_for_fetch_git);

        let args = attrset! {
            url: self.url,
            rev: self.rev,
            submodules: true,
        }
        .merge(match r#ref {
            Some(r#ref) => Either::Left(attrset! { ref: r#ref }),
            None => Either::Right(attrset! { allRefs: true }),
        });

        let add_checksum_args = attrset! {
            src: fetch_git.call(args, ctx)?,
            name: format!("git-{}", &self.rev[..8]),
            runCommandLocal: run_command_local,
        };

        let add_checksum = WRAP.with(|cell| match cell.get().copied() {
            Some(wrap) => Ok::<_, NixError>(wrap),
            None => {
                let wrap = ctx.eval::<NixLambda>(
                    c"
                    { src, name, runCommandLocal }:
                    runCommandLocal name {} ''
                      cp -r ${src} $out
                      chmod +w $out
                      echo '{\"package\":null,\"files\":{}}' > $out/.cargo-checksum.json
                    ''
                ")?;
                Ok(*cell.get_or_init(|| wrap))
            },
        })?;

        add_checksum.call(add_checksum_args, ctx)
    }
}

impl Function for VendorDeps {
    type Args<'a> = VendorDepsArgs<'a>;

    fn call<'a: 'a>(
        args: Self::Args<'a>,
        ctx: &mut Context,
    ) -> Result<NixDerivation<'static>, VendorDepsError> {
        let cargo_lock = Self::read_cargo_lock(&args.cargo_lock)?;
        let sources = VendoredSources::new(&cargo_lock, args.pkgs, ctx)?;
        sources.to_dir(args.pkgs, ctx).map_err(Into::into)
    }
}

impl fmt::Display for SourceId<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.package_name, self.version)
    }
}

impl PartialEq for SourceId<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for SourceId<'_> {}

impl PartialOrd for SourceId<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SourceId<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.package_name
            .cmp(other.package_name)
            .then_with(|| self.version.cmp(&other.version))
    }
}

impl From<VendorDepsError> for NixError {
    fn from(err: VendorDepsError) -> Self {
        match err {
            VendorDepsError::Nix(nix_err) => nix_err,
            other => {
                let message = CString::new(other.to_string())
                    .expect("the Display impl doesn't contain any NUL bytes");
                Self::new(ErrorKind::Nix, message)
            },
        }
    }
}
