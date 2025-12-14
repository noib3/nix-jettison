use core::cmp::Ordering;
use core::fmt::Display;
use core::mem;
use core::ops::Not;
use std::collections::{HashMap, hash_map};

use cargo::core::compiler::CrateType;
use cargo::core::dependency::DepKind;
use cargo::core::manifest::TargetSourcePath;
use cargo::core::{Package, PackageId, Target, TargetKind};
use cargo_util_schemas::manifest::TomlPackageBuild;
use compact_str::{CompactString, ToCompactString};
use nix_bindings::prelude::*;

use crate::resolve_build_graph::WorkspaceResolve;

/// The crate-specific arguments accepted by
/// [`pkgs.buildRustCrate`][buildRustCrate].
///
/// [buildRustCrate]: https://github.com/NixOS/nixpkgs/blob/d792a6e0cd4ba35c90ea787b717d72410f56dc40/pkgs/build-support/rust/build-rust-crate/default.nix
#[derive(nix_bindings::Attrset)]
#[attrset(rename_all = camelCase)]
pub(crate) struct BuildCrateArgs {
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

    /// TODO: docs.
    pub(crate) crate_name: CompactString,

    /// This is derived state from the dependencies section of the Cargo.toml
    /// of the crate.
    #[attrset(skip_if = HashMap::is_empty)]
    pub(crate) crate_renames: HashMap<CompactString, CrateRename>,

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

    /// TODO: docs.
    pub(crate) version: CompactString,
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

#[derive(cauchy::Default, nix_bindings::Attrset)]
#[attrset(bounds = { Dep: ToValue })]
pub(crate) struct Dependencies<Dep> {
    #[attrset(rename = "dependencies", skip_if = Vec::is_empty)]
    pub(crate) normal: Vec<Dep>,

    #[attrset(rename = "buildDependencies", skip_if = Vec::is_empty)]
    pub(crate) build: Vec<Dep>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SourceId<'a> {
    pub(crate) name: &'a str,
    pub(crate) version: &'a str,
}

impl BuildCrateArgs {
    #[allow(clippy::too_many_lines)]
    pub(crate) fn new(package: &Package, resolve: &WorkspaceResolve) -> Self {
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
            crate_name: CompactString::const_new(package.name().as_str()),
            crate_renames: Self::new_crate_renames(package_id, resolve),
            // Replace newlines and escape double quotes because buildRustCrate
            // exports the description as a bash environment variable without
            // proper escaping, which breaks when the description contains
            // newlines or double quotes.
            // See https://github.com/NixOS/nixpkgs/blob/d792a6e0cd4ba35c90ea787b717d72410f56dc40/pkgs/build-support/rust/build-rust-crate/configure-crate.nix#L144
            description: metadata
                .description
                .as_deref()
                .map(|s| s.replace('\n', " ").replace('"', "\\\"").into()),
            edition: Some(manifest.edition().to_compact_string()),
            extra_rustc_opts: Vec::new(),
            extra_rustc_opts_for_build_rs: Vec::new(),
            features: resolve.features(package_id).map(Into::into).collect(),
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
            version: package.version().to_compact_string(),
        }
    }

    fn new_crate_renames(
        package_id: PackageId,
        resolve: &WorkspaceResolve,
    ) -> HashMap<CompactString, CrateRename> {
        let mut renames = HashMap::new();

        for (_dep_id, dep_set) in resolve.deps(package_id) {
            for dep in dep_set {
                // Skip dev-dependencies since we're not building tests.
                if dep.kind() == DepKind::Development {
                    continue;
                }

                let Some(name_in_toml) = dep.explicit_name_in_toml() else {
                    continue;
                };

                let rename_with_version = CrateRenameWithVersion {
                    rename: CompactString::const_new(name_in_toml.as_str()),
                    version: dep.version_req().to_compact_string(),
                };

                match renames.entry(dep.package_name().as_str().into()) {
                    hash_map::Entry::Occupied(mut entry) => {
                        let CrateRename::Extended(versions) = entry.get_mut()
                        else {
                            unreachable!(
                                "we only create extended renames on the first \
                                 pass"
                            );
                        };
                        versions.push(rename_with_version);
                    },
                    hash_map::Entry::Vacant(entry) => {
                        // TODO: use smallvec with an inline capacity of 1.
                        entry.insert(CrateRename::Extended(vec![
                            rename_with_version,
                        ]));
                    },
                }
            }
        }

        renames.values_mut().for_each(|rename| {
            let CrateRename::Extended(versions) = rename else { return };
            if versions.len() > 1 {
                return;
            }
            let name_in_toml = mem::take(versions)
                .into_iter()
                .next()
                .expect("checked length")
                .rename;
            *rename = CrateRename::Simple(name_in_toml);
        });

        renames
    }

    pub(crate) fn source_id(&self) -> SourceId<'_> {
        SourceId { name: &self.crate_name, version: &self.version }
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

impl Display for SourceId<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}-{}", self.name, self.version)
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
        self.name.cmp(other.name).then_with(|| self.version.cmp(other.version))
    }
}
