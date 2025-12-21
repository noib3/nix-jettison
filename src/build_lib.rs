use core::ops::Not;
use std::collections::HashMap;
use std::env::consts::DLL_EXTENSION;

use cargo::core::Edition;
use cargo::core::compiler::CompileTarget;
use compact_str::{CompactString, format_compact};
use indoc::formatdoc;
use nix_bindings::prelude::*;

#[derive(nix_bindings::Attrset)]
#[attrset(rename_all = camelCase)]
pub(crate) struct BuildLibArgs {
    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) authors: Vec<String>,

    /// This is derived state which can be specified in Cargo profiles (for
    /// example: `[profile.release] codegen-units = N`).
    #[attrset(skip_if = Option::is_none)]
    pub(crate) codegen_units: Option<u32>,

    /// This is derived state from the dependencies section of the Cargo.toml
    /// of the crate.
    #[attrset(skip_if = HashMap::is_empty)]
    pub(crate) crate_renames: HashMap<CompactString, CrateRename>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) description: Option<CompactString>,

    /// The Rust edition specified by the package this library is in.
    #[attrset(with_value = |&ed| edition_as_str(ed))]
    pub(crate) edition: Edition,

    /// Extra command-line arguments to pass to `rustc` when building this
    /// library.
    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) extra_rustc_args: Vec<CompactString>,

    /// The list of features to enable when building this library.
    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) features: Vec<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) homepage: Option<CompactString>,

    /// Whether this library is a procedural macro.
    #[attrset(skip_if = Not::not)]
    pub(crate) is_proc_macro: bool,

    /// The name of the library target. This is usually the
    /// [`package_name`](Self::package_name) with dashes replaced by
    /// underscores.
    #[attrset(skip_if = |name| name == self.package_name.replace("-", "_"))]
    pub(crate) lib_name: CompactString,

    /// The path to the entrypoint of the library's module tree from the root
    /// of the package, (usually `src/lib.rs`).
    #[attrset(skip_if = |path| path == "src/lib.rs")]
    pub(crate) lib_path: CompactString,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) license_file: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) links: Option<CompactString>,

    /// The name of the package this library is in.
    pub(crate) package_name: CompactString,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) readme: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) repository: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) rust_version: Option<CompactString>,

    /// The version of the package this library is in.
    pub(crate) version: CompactString,

    /// TODO: this is used by `buildRustCrate` to `cd` from the `src`
    /// directory. We should only set for Git dependencies when the path from
    /// the repo's root to the package root is non-empty.
    #[attrset(rename = "workspace_member", skip_if = Option::is_none)]
    pub(crate) workspace_member: Option<CompactString>,
}

#[derive(nix_bindings::Value)]
pub(crate) enum CrateRename {
    Simple(CompactString),
    Extended(Vec<CrateRenameWithVersion>),
}

/// Represents a version-specific rename for the extended crateRenames format.
#[derive(nix_bindings::Attrset)]
pub(crate) struct CrateRenameWithVersion {
    pub(crate) rename: CompactString,
    pub(crate) version: CompactString,
}

/// TODO: docs.
#[derive(nix_bindings::Attrset, nix_bindings::TryFromValue)]
#[attrset(rename_all = camelCase)]
#[try_from(rename_all = camelCase)]
struct MkDerivationPassthroughArgs {
    is_proc_macro: bool,
    lib_name: CompactString,
    package_name: CompactString,
    version: CompactString,
}

impl BuildLibArgs {
    /// The relative path to the output directory where the built library files
    /// will be placed from the root of the build directory.
    const OUT_DIR: &'static str = "target/lib";

    pub(crate) fn to_mk_derivation_args<'dep, Src: Value, Drv: ToValue>(
        &self,
        src: Src,
        build_inputs: &[Drv],
        native_build_inputs: &[Drv],
        dependencies: impl IntoIterator<Item = NixDerivation<'dep>>,
        release: bool,
        ctx: &mut Context,
    ) -> impl Attrset + Value {
        let rustc_args = self.rustc_args(release, dependencies, None, ctx);

        attrset! {
            name: format_compact!("{}-{}-lib", self.package_name, self.version),
            version: &*self.version,
            src,
            buildInputs: build_inputs,
            nativeBuildInputs: native_build_inputs,
            configurePhase: formatdoc!("
                runHook preConfigure
                # TODO: add symlinks to link library dependencies
                # TODO: source env files produced by build scripts of direct
                # dependencies (only if `links` is set for those dependencies),
                # see https://doc.rust-lang.org/cargo/reference/build-scripts.html#the-links-manifest-key
                # TODO: set `CARGO_PKG` and `CARGO_CFG` env vars
                runHook postConfigure
            "),
            buildPhase: formatdoc!("
                runHook preBuild
                rustc {}
                runHook postBuild
            ", rustc_args.into_iter().fold(String::new(), |mut args, arg| {
                args.push(' ');
                args.push_str(arg.as_ref());
                args
            })),
            installPhase: formatdoc!("
                runHook preInstall
                mkdir -p $lib/lib
                cp -r {}/* $lib/lib
                runHook postInstall
            ", Self::OUT_DIR),
            dontStrip: false,
            stripExclude: [ c"*.rlib" ].into_value(),
            outputs: [ c"out", c"lib" ].into_value(),
            passthrough: MkDerivationPassthroughArgs {
                is_proc_macro: self.is_proc_macro,
                lib_name: self.lib_name.clone(),
                package_name: self.package_name.clone(),
                version: self.version.clone(),
            },
        }
    }

    /// Returns an iterator over the `--extern {name}={path}` command-line
    /// arguments for the given dependencies to pass to `rustc`.
    fn dependencies_args<'dep>(
        &self,
        dependencies: impl IntoIterator<Item = NixDerivation<'dep>>,
        ctx: &mut Context,
    ) -> impl IntoIterator<Item = CompactString> {
        dependencies
            .into_iter()
            .map(|dep_drv| {
                let dep = dep_drv
                    .get::<MkDerivationPassthroughArgs>(c"passthrough", ctx)
                    .expect("dependency must have passthrough args");

                let lib_name =
                    match self.crate_renames.get(&dep.package_name) {
                        Some(CrateRename::Simple(rename)) => rename,
                        Some(CrateRename::Extended(renames)) => renames
                            .iter()
                            .find_map(
                                |CrateRenameWithVersion { rename, version }| {
                                    (version == dep.version).then(|| rename)
                                },
                            )
                            .unwrap_or_else(|| &dep.lib_name),
                        None => &dep.lib_name,
                    }
                    .clone();

                let out_path = dep_drv
                    .out_path(ctx)
                    .expect("dependency derivation must have an output path");

                let lib_path = format!(
                    "{}/lib{}.{}",
                    out_path.display(),
                    dep.lib_name,
                    if dep.is_proc_macro { DLL_EXTENSION } else { "rlib" }
                );

                (lib_name, lib_path)
            })
            .flat_map(|(lib_name, lib_path)| {
                [
                    CompactString::const_new("--extern"),
                    format_compact!("{}={}", lib_name, lib_path),
                ]
            })
    }

    /// Returns the list of command-line arguments to pass to `rustc` to build
    /// this library.
    fn rustc_args<'dep>(
        &self,
        release: bool,
        dependencies: impl IntoIterator<Item = NixDerivation<'dep>>,
        compile_target: Option<CompileTarget>,
        ctx: &mut Context,
    ) -> impl IntoIterator<Item = impl AsRef<str>> {
        [
            self.lib_path.as_str(),
            "--crate-name",
            self.lib_name.as_str(),
            "--crate-type lib",
            "--out-dir",
            Self::OUT_DIR,
            "--edition",
            edition_as_str(self.edition),
            "--cap-lints allow", // Suppress all lints from dependencies.
            "--remap-path-prefix=$NIX_BUILD_TOP=/",
            "--colors always",
            "-C",
            if release { "opt-level=3" } else { "debuginfo=2" },
            "-C",
        ]
        .into_iter()
        .map(Into::into)
        .chain([format_compact!(
            "codegen-units={}",
            self.codegen_units.unwrap_or(1)
        )])
        .chain(
            self.is_proc_macro
                .then(|| CompactString::const_new("--extern proc-macro")),
        )
        .chain(self.dependencies_args(dependencies, ctx))
        .chain(
            (match compile_target {
                // Proc-macros run on the host, so we don't set a target for
                // them.
                Some(target) if !self.is_proc_macro => Some([
                    CompactString::const_new("--target"),
                    target.rustc_target().as_str().into(),
                ]),
                _ => None,
            })
            .into_iter()
            .flatten(),
        )
        .chain(self.features.iter().flat_map(|feature| {
            [
                CompactString::const_new("--cfg"),
                format_compact!("feature=\"{}\"", feature),
            ]
        }))
        // TODO: set linker.
        .chain(self.extra_rustc_args.iter().cloned())
    }
}

#[inline]
fn edition_as_str(edition: Edition) -> &'static str {
    match edition {
        Edition::Edition2015 => "2015",
        Edition::Edition2018 => "2018",
        Edition::Edition2021 => "2021",
        Edition::Edition2024 => "2024",
        Edition::EditionFuture => "future",
    }
}
