use core::ops::Not;
use std::borrow::Cow;
use std::path::Path;

use cargo::core::compiler::CrateType;
use cargo::core::manifest::TargetSourcePath;
use cargo::core::{Package, Resolve, Target, TargetKind};
use cargo_util_schemas::manifest::TomlPackageBuild;
use compact_str::{CompactString, ToCompactString};
use nix_bindings::prelude::*;

use crate::resolve_build_graph::ResolveBuildGraphArgs;
use crate::vendor_deps::VendorDir;

/// The arguments accepted by [`pkgs.buildRustCrate`][buildRustCrate].
///
/// [buildRustCrate]: https://github.com/NixOS/nixpkgs/blob/d792a6e0cd4ba35c90ea787b717d72410f56dc40/pkgs/build-support/rust/build-rust-crate/default.nix
pub(crate) struct BuildCrateArgs<'src, Dep> {
    pub(crate) required: RequiredBuildCrateArgs<'src>,
    pub(crate) optional: OptionalBuildCrateArgs,
    pub(crate) dependencies: Dependencies<Dep>,
}

/// The required, crate-specific arguments accepted by `pkgs.buildRustCrate`.
#[derive(nix_bindings::Attrset)]
#[attrset(rename_all = camelCase)]
pub(crate) struct RequiredBuildCrateArgs<'src> {
    pub(crate) crate_name: CompactString,
    #[attrset(with_value = Self::src_path)]
    pub(crate) src: CrateSource<'src>,
    pub(crate) version: CompactString,
}

/// The path to a crate's source directory.
pub(crate) enum CrateSource<'src> {
    /// The crate's source is at the given path.
    Path(Cow<'src, Path>),

    /// The crate is a 3rd-party dependency which has been vendored under the
    /// given directory.
    Vendored(Cow<'src, Path>),
}

/// The optional, crate-specific arguments accepted by `pkgs.buildRustCrate`.
#[derive(Default, nix_bindings::Attrset)]
#[attrset(rename_all = camelCase)]
pub(crate) struct OptionalBuildCrateArgs {
    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) authors: Vec<String>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) build: Option<CompactString>,

    /// This is derived state which can be specified in Cargo profiles (for
    /// example: `[profile.release] codegen-units = N`).
    #[attrset(skip_if = Option::is_none)]
    pub(crate) codegen_units: Option<u32>,

    /// This is derived state from the Cargo.toml/source structure of the
    /// crate.
    #[attrset(skip_if = Option::is_none)]
    pub(crate) crate_bin: Option<Null>,

    /// This is derived state from the dependencies section of the Cargo.toml
    /// of the crate.
    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) crate_renames: Vec<Null>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) description: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) edition: Option<CompactString>,

    /// This is derived state from the Cargo config.
    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) extra_rustc_opts: Vec<CompactString>,

    /// This is derived state from the Cargo config (get by checking e.g.
    /// `profile.release.build-override`, so it can differ from
    /// `extra_rustc_opts`).
    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) extra_rustc_opts_for_build_rs: Vec<CompactString>,

    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) features: Vec<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) homepage: Option<CompactString>,

    #[attrset(rename = "type", skip_if = Vec::is_empty)]
    pub(crate) lib_crate_types: Vec<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) lib_name: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) lib_path: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) license_file: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) links: Option<CompactString>,

    #[attrset(skip_if = Not::not)]
    pub(crate) proc_macro: bool,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) readme: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) repository: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) rust_version: Option<CompactString>,
}

#[derive(cauchy::Default, nix_bindings::Attrset)]
#[attrset(bounds = { Dep: ToValue })]
pub(crate) struct Dependencies<Dep> {
    #[attrset(rename = "dependencies", skip_if = Vec::is_empty)]
    pub(crate) normal: Vec<Dep>,

    #[attrset(rename = "buildDependencies", skip_if = Vec::is_empty)]
    pub(crate) build: Vec<Dep>,
}

impl<'src, Dep> BuildCrateArgs<'src, Dep> {
    pub(crate) fn map_deps<NewDep>(
        self,
        fun: impl FnMut(Dep) -> NewDep + Clone,
    ) -> BuildCrateArgs<'src, NewDep> {
        BuildCrateArgs {
            required: self.required,
            optional: self.optional,
            dependencies: self.dependencies.map(fun),
        }
    }

    pub(crate) fn to_attrset(&self) -> impl Attrset + Value
    where
        Dep: Value,
    {
        // SAFETY: the inner attrsets don't contain any overlapping keys.
        unsafe {
            self.required
                .borrow()
                .concat(self.optional.borrow())
                .concat(self.dependencies.borrow())
        }
    }
}

impl<'src> RequiredBuildCrateArgs<'src> {
    pub(crate) fn new(
        package: &Package,
        args: &ResolveBuildGraphArgs<'src>,
    ) -> Self {
        Self {
            crate_name: package.name().as_str().into(),
            src: CrateSource::new(package, args),
            version: package.version().to_compact_string(),
        }
    }

    fn src_path(&self) -> Cow<'_, Path> {
        self.src.to_path(&self.crate_name, &self.version)
    }
}

impl<'src> CrateSource<'src> {
    fn new(package: &Package, args: &ResolveBuildGraphArgs<'src>) -> Self {
        if package.package_id().source_id().is_path() {
            let package_root = package.root();
            let path = if package_root == args.src {
                Cow::Borrowed(args.src)
            } else {
                Cow::Owned(package.root().to_owned())
            };
            Self::Path(path)
        } else {
            Self::Vendored(args.vendor_dir.clone())
        }
    }

    fn to_path<'a>(&'a self, crate_name: &str, version: &str) -> Cow<'a, Path> {
        match self {
            Self::Path(path) => Cow::Borrowed(&**path),
            Self::Vendored(vendor_dir) => Cow::Owned(
                vendor_dir.join(VendorDir::dir_name(crate_name, version)),
            ),
        }
    }
}

impl OptionalBuildCrateArgs {
    #[allow(clippy::too_many_lines)]
    pub(crate) fn new(package: &Package, resolve: &Resolve) -> Self {
        let manifest = package.manifest();
        let metadata = manifest.metadata();
        let package_id = package.package_id();

        let lib_target = package
            .targets()
            .iter()
            // A package cannot have multiple library targets, so we can
            // stop iterating after finding the first one.
            .find_map(|target| match target.kind() {
                TargetKind::Lib(crate_types) => Some((target, &**crate_types)),
                _ => None,
            });

        Self {
            authors: metadata.authors.clone(),
            build: manifest
                .original_toml()
                .package()
                .and_then(|pkg| pkg.build.as_ref())
                .and_then(|pkg_build| match pkg_build {
                    TomlPackageBuild::Auto(_) => None,
                    TomlPackageBuild::SingleScript(str) => Some((**str).into()),
                    TomlPackageBuild::MultipleScript(_) => None,
                }),
            codegen_units: None,
            crate_bin: None,
            crate_renames: Vec::new(),
            description: metadata.description.as_deref().map(Into::into),
            edition: Some(manifest.edition().to_compact_string()),
            extra_rustc_opts: Vec::new(),
            extra_rustc_opts_for_build_rs: Vec::new(),
            features: resolve
                .features(package_id)
                .iter()
                .map(|feat| feat.as_str().into())
                .collect(),
            homepage: metadata.homepage.as_deref().map(Into::into),
            lib_crate_types: lib_target
                .map_or(&[][..], |(_target, crate_types)| crate_types)
                .iter()
                .filter_map(|crate_type| match crate_type {
                    // Filter out Lib, buildRustCrate already defaults to
                    // ["lib"] if we pass an empty list.
                    //
                    // See https://github.com/NixOS/nixpkgs/blob/d792a6e0cd4ba35c90ea787b717d72410f56dc40/pkgs/build-support/rust/build-rust-crate/default.nix#L373
                    CrateType::Lib => None,
                    other => Some(other.as_str().into()),
                })
                .collect(),
            lib_name: lib_target.and_then(|(lib_target, _crate_types)| {
                // Only set the library name if it differs from the package
                // name.
                (lib_target.name() != package.name().as_str())
                    .then_some(lib_target.name().into())
            }),
            lib_path: lib_target
                .and_then(|(lib_target, _crate_types)| {
                    match lib_target.src_path() {
                        TargetSourcePath::Path(path) => Some(&**path),
                        TargetSourcePath::Metabuild => None,
                    }
                })
                .and_then(|lib_path| {
                    let lib_path_relative = lib_path
                        .strip_prefix(package.root())
                        .expect("library path is under package root");
                    (lib_path_relative != "src/lib.rs").then(|| {
                        lib_path_relative.display().to_compact_string()
                    })
                }),
            license_file: metadata.license_file.as_deref().map(Into::into),
            links: metadata.links.as_deref().map(Into::into),
            proc_macro: package.targets().iter().any(Target::proc_macro),
            readme: metadata.readme.as_deref().map(Into::into),
            repository: metadata.repository.as_deref().map(Into::into),
            rust_version: metadata
                .rust_version
                .as_ref()
                .map(|v| v.to_compact_string()),
        }
    }
}

impl<Dep> Dependencies<Dep> {
    pub(crate) fn map<NewDep>(
        self,
        fun: impl FnMut(Dep) -> NewDep + Clone,
    ) -> Dependencies<NewDep> {
        Dependencies {
            build: self.build.into_iter().map(fun.clone()).collect(),
            normal: self.normal.into_iter().map(fun).collect(),
        }
    }
}

impl<Dep: Value> ToValue for BuildCrateArgs<'_, Dep> {
    fn to_value<'a>(&'a self, _: &mut Context) -> impl Value + use<'a, Dep> {
        self.to_attrset()
    }
}
