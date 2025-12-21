use core::iter;
use std::collections::HashMap;
use std::env::consts::DLL_EXTENSION;

use cargo::core::Edition;
use cargo::core::compiler::CompileTarget;
use compact_str::{CompactString, format_compact};
use either::Either;
use indoc::formatdoc;
use nix_bindings::prelude::*;
use smallvec::SmallVec;

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

    #[attrset(skip_if = Option::is_none)]
    pub(crate) license_file: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) links: Option<CompactString>,

    /// The name of the package this crate is in.
    pub(crate) package_name: CompactString,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) readme: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) repository: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) rust_version: Option<CompactString>,

    pub(crate) r#type: DerivationType,

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

pub(crate) enum DerivationType {
    /// The derivation will build one or more binary crates.
    Bin(SmallVec<[BinCrate; 1]>),

    /// The derivation will build a single library crate.
    Lib(LibCrate),

    /// The derivation will build and run the build script at the given path
    /// relative to the package root.
    BuildScript(CompactString),
}

#[derive(nix_bindings::Attrset)]
#[attrset(rename_all = camelCase)]
pub(crate) struct BinCrate {
    name: CompactString,
    path: CompactString,
    #[attrset(skip_if = Vec::is_empty)]
    required_features: Vec<CompactString>,
}

#[derive(nix_bindings::Attrset)]
#[attrset(rename_all = camelCase)]
pub(crate) struct LibCrate {
    /// The name of the library target. This is usually the
    /// [`package_name`](BuildLibArgs::package_name) with dashes replaced by
    /// underscores.
    pub(crate) name: CompactString,

    /// The path to the entrypoint of the library's module tree from the root
    /// of the package, (usually `src/lib.rs`).
    pub(crate) path: CompactString,

    /// The library formats to generate when building this crate.
    #[attrset(skip_if = SmallVec::is_empty)]
    pub(crate) formats: SmallVec<[LibFormat; 1]>,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum LibFormat {
    Cdylib,
    Dylib,
    Lib,
    ProcMacro,
    Rlib,
    Staticlib,
}

enum CrateType<'a> {
    Bin(&'a BinCrate),
    Lib(&'a LibCrate),
    BuildScript(&'a CompactString),
}

/// TODO: docs.
#[derive(nix_bindings::Attrset, nix_bindings::TryFromValue)]
#[attrset(rename_all = camelCase)]
#[try_from(rename_all = camelCase)]
struct MkDerivationPassthroughArgs {
    is_proc_macro: bool,
    lib_name: Option<CompactString>,
    package_name: CompactString,
    version: CompactString,
}

impl DerivationType {
    fn is_proc_macro(&self) -> bool {
        match self {
            DerivationType::Lib(lib_crate) => lib_crate.is_proc_macro(),
            _ => false,
        }
    }
}

impl LibCrate {
    fn is_proc_macro(&self) -> bool {
        &*self.formats == &[LibFormat::ProcMacro]
    }
}

impl LibFormat {
    fn as_str(self) -> &'static str {
        match self {
            LibFormat::Cdylib => "cdylib",
            LibFormat::Dylib => "dylib",
            LibFormat::Lib => "lib",
            LibFormat::ProcMacro => "proc-macro",
            LibFormat::Rlib => "rlib",
            LibFormat::Staticlib => "staticlib",
        }
    }
}

impl<'a> CrateType<'a> {
    /// Returns the argument to pass to `--crate-name` for this crate type.
    fn crate_name_arg(&self) -> &'a str {
        match self {
            Self::Bin(bin_crate) => &bin_crate.name,
            Self::Lib(lib_crate) => &lib_crate.name,
            Self::BuildScript(_) => "build_script_build",
        }
    }

    /// Returns the argument to pass to `--crate-type` for this crate type.
    fn crate_type_arg(&self) -> CompactString {
        match self {
            Self::Bin(_) | Self::BuildScript(_) => {
                CompactString::const_new("bin")
            },
            Self::Lib(lib_crate) => lib_crate.formats.iter().fold(
                CompactString::default(),
                |mut acc, format| {
                    if !acc.is_empty() {
                        acc.push(',');
                    }
                    acc.push_str(format.as_str());
                    acc
                },
            ),
        }
    }

    fn is_compiled_for_host(&self) -> bool {
        match self {
            CrateType::Bin(_) => true,
            // Proc macros run on the build machine.
            CrateType::Lib(lib_crate) => !lib_crate.is_proc_macro(),
            // Build scripts run on the build machine.
            CrateType::BuildScript(_) => false,
        }
    }

    /// Returns the path argument to pass as the input source file to `rustc`
    /// for this crate type.
    fn path_arg(&self) -> &'a str {
        match self {
            Self::Bin(bin_crate) => &bin_crate.path,
            Self::Lib(lib_crate) => &lib_crate.path,
            Self::BuildScript(path) => &**path,
        }
    }
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
        dependencies: impl Iterator<Item = NixDerivation<'dep>> + Clone,
        release: bool,
        ctx: &mut Context,
    ) -> impl Attrset + Value {
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
            buildPhase: self.build_phase(
                release,
                dependencies,
                None,
                ctx,
            ),
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
                is_proc_macro: self.r#type.is_proc_macro(),
                lib_name: match &self.r#type {
                    DerivationType::Lib(lib) => Some(lib.name.clone()),
                    _ => None,
                },
                package_name: self.package_name.clone(),
                version: self.version.clone(),
            },
        }
    }

    fn build_phase<'dep>(
        &self,
        release: bool,
        dependencies: impl Iterator<Item = NixDerivation<'dep>> + Clone,
        compile_target: Option<&CompileTarget>,
        ctx: &mut Context,
    ) -> String {
        let crate_types = match &self.r#type {
            DerivationType::Bin(bins) => {
                Either::Right(bins.iter().map(CrateType::Bin))
            },
            DerivationType::Lib(lib) => {
                Either::Left(iter::once(CrateType::Lib(&lib)))
            },
            DerivationType::BuildScript(path) => {
                Either::Left(iter::once(CrateType::BuildScript(path)))
            },
        };

        let mut build_phase = "runHook preBuild".to_owned();

        for crate_type in crate_types {
            build_phase.push_str("\nrustc");

            for rustc_arg in self.build_rustc_args(
                release,
                crate_type,
                dependencies.clone(),
                compile_target,
                ctx,
            ) {
                build_phase.push(' ');
                build_phase.push_str(rustc_arg.as_ref());
            }
        }

        build_phase.push_str("\nrunHook postBuild");

        build_phase
    }

    /// Returns the list of command-line arguments to pass to `rustc` to build
    /// this library.
    fn build_rustc_args<'dep>(
        &self,
        release: bool,
        crate_type: CrateType<'_>,
        dependencies: impl Iterator<Item = NixDerivation<'dep>>,
        compile_target: Option<&CompileTarget>,
        ctx: &mut Context,
    ) -> impl IntoIterator<Item = impl AsRef<str>> {
        [
            crate_type.path_arg(),
            "--crate-name",
            crate_type.crate_name_arg(),
            "--out-dir",
            Self::OUT_DIR,
            "--edition",
            edition_as_str(self.edition),
            "--cap-lints allow", // Suppress all lints from dependencies.
            "--remap-path-prefix $NIX_BUILD_TOP=/",
            "--colors always",
            "--codegen",
            if release { "opt-level=3" } else { "debuginfo=2" },
            "--codegen",
        ]
        .into_iter()
        .map(Into::into)
        .chain([
            format_compact!(
                "codegen-units={}",
                self.codegen_units.unwrap_or(1)
            ),
            CompactString::const_new("--crate-type"),
            crate_type.crate_type_arg(),
        ])
        .chain(
            self.r#type
                .is_proc_macro()
                .then(|| CompactString::const_new("--extern proc-macro")),
        )
        .chain(self.dependencies_args(dependencies, ctx))
        .chain(
            (match compile_target {
                Some(target) if crate_type.is_compiled_for_host() => Some([
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

                let dep_lib_name = dep
                    .lib_name
                    .as_ref()
                    .expect("only library crates can be dependencies");

                let lib_name =
                    match self.crate_renames.get(&dep.package_name) {
                        Some(CrateRename::Simple(rename)) => rename,
                        Some(CrateRename::Extended(renames)) => renames
                            .iter()
                            .find_map(|rename| {
                                (rename.version == dep.version)
                                    .then(|| &rename.rename)
                            })
                            .unwrap_or_else(|| dep_lib_name),
                        None => dep_lib_name,
                    }
                    .clone();

                let out_path = dep_drv
                    .out_path(ctx)
                    .expect("dependency derivation must have an output path");

                let lib_path = format!(
                    "{}/lib{}.{}",
                    out_path.display(),
                    dep_lib_name,
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
}

impl ToValue for DerivationType {
    fn to_value<'this, 'eval>(
        &'this self,
        ctx: &mut Context<'eval>,
    ) -> impl Value + use<'this, 'eval> {
        match self {
            DerivationType::Bin(bin_crates) => Either::Left(attrset! {
                type: c"bin",
                binCrates: bin_crates.to_value(ctx),
            }),
            DerivationType::Lib(lib_crate) => {
                Either::Right(Either::Left(attrset! {
                    type: c"lib",
                    libCrate: lib_crate.to_value(ctx),
                }))
            },
            DerivationType::BuildScript(path) => {
                Either::Right(Either::Right(attrset! {
                    type: c"build-script",
                    path: path.to_value(ctx),
                }))
            },
        }
    }
}

impl ToValue for LibFormat {
    fn to_value(&self, _: &mut Context) -> impl Value + use<> {
        self.as_str()
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
