use core::cmp::Ordering;
use core::fmt;
use std::borrow::Cow;
use std::env::consts::DLL_EXTENSION;

use cargo::core::compiler::CompileTarget;
use compact_str::{CompactString, format_compact};
use nix_bindings::prelude::*;
use smallvec::SmallVec;

#[derive(Clone)]
pub(crate) enum NodeType {
    /// The derivation will build one or more binary crates.
    Bin(SmallVec<[BinCrate; 1]>),

    /// The derivation will build a single library crate.
    Lib(LibCrate),

    /// The derivation will build and run the build script at the given path
    /// relative to the package root.
    BuildScript(CompactString),
}

#[derive(nix_bindings::Attrset, Clone)]
#[attrset(rename_all = camelCase)]
pub(crate) struct LibCrate {
    /// The name of the library target. This is usually the
    /// [`package_name`](BuildNodeInfos::package_name) with dashes replaced by
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

#[derive(Clone, Debug)]
pub(crate) struct SourceId<'a> {
    pub(crate) package_name: &'a str,
    pub(crate) version: Cow<'a, str>,
}

pub(crate) enum CrateType<'a> {
    Bin(&'a BinCrate),
    Lib(&'a LibCrate),
    BuildScript(&'a CompactString),
}

impl NodeType {
    pub(crate) fn is_build_script(&self) -> bool {
        matches!(self, NodeType::BuildScript(_))
    }

    pub(crate) fn is_proc_macro(&self) -> bool {
        match self {
            NodeType::Lib(lib_crate) => lib_crate.is_proc_macro(),
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

impl BuildNodeAttrs {
    /// Returns the list of command-line arguments to pass to `rustc` to build
    /// this library.
    pub(crate) fn build_rustc_args<'dep>(
        &self,
        release: bool,
        crate_type: CrateType<'_>,
        dependencies: impl Iterator<Item = (&'dep Self, NixDerivation<'dep>)>,
        compile_target: Option<&CompileTarget>,
        ctx: &mut Context,
    ) -> impl IntoIterator<Item = impl AsRef<str>> {
        [
            crate_type.path_arg(),
            "--crate-name",
            crate_type.crate_name_arg(),
            "--out-dir",
            self.out_dir(),
            "--edition",
            edition_as_str(self.edition),
            "--cap-lints allow", // Suppress all lints from dependencies.
            "--remap-path-prefix $NIX_BUILD_TOP=/",
            "--color always",
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
                format_compact!("feature=\\\"{}\\\"", feature),
            ]
        }))
        // TODO: set linker.
        .chain(self.extra_rustc_args.iter().cloned())
    }

    /// Returns the name of the derivation for this build node.
    pub(crate) fn derivation_name(&self) -> CompactString {
        let name_suffix = match &self.r#type {
            NodeType::Bin(crates) if crates.len() > 1 => "bins",
            NodeType::Bin(_) => "bin",
            NodeType::Lib(_) => "lib",
            NodeType::BuildScript(_) => "build",
        };

        format_compact!(
            "{}-{}-{}",
            self.package_name,
            self.version,
            name_suffix
        )
    }

    /// Returns the relative path from the root of the build directory to the
    /// directory containing the build artifacts.
    pub(crate) fn out_dir(&self) -> &'static str {
        match &self.r#type {
            NodeType::Bin(_) => "target/bin",
            NodeType::Lib(_) => "target/lib",
            NodeType::BuildScript(_) => "target/build",
        }
    }

    /// Returns an iterator over the `--extern {name}={path}` command-line
    /// arguments for the given dependencies to pass to `rustc`.
    fn dependencies_args<'dep>(
        &self,
        dependencies: impl Iterator<Item = (&'dep Self, NixDerivation<'dep>)>,
        ctx: &mut Context,
    ) -> impl IntoIterator<Item = CompactString> {
        dependencies
            .into_iter()
            .map(|(dep_args, dep_drv)| {
                let dep_lib_name = match dep_args.r#type {
                    NodeType::Lib(ref lib_crate) => &lib_crate.name,
                    _ => panic!("only library crates can be dependencies"),
                };

                let lib_name =
                    match self.crate_renames.get(&dep_args.package_name) {
                        Some(DependencyRename::Simple(rename)) => rename,
                        Some(DependencyRename::Extended(renames)) => renames
                            .iter()
                            .find_map(|rename| {
                                (rename.version == dep_args.version)
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
                    if dep_args.r#type.is_proc_macro() {
                        DLL_EXTENSION
                    } else {
                        "rlib"
                    }
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
