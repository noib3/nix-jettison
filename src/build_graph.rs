use core::mem;
use std::borrow::Cow;
use std::collections::{HashMap, hash_map};
use std::path::PathBuf;

use cargo::core::compiler::{CompileKind, CrateType};
use cargo::core::dependency::DepKind;
use cargo::core::manifest::TargetSourcePath;
use cargo::core::profiles::UnitFor;
use cargo::core::{Edition, Package, PackageId, TargetKind};
use cargo::util::OptVersionReq;
use cargo_util_schemas::manifest::TomlPackageBuild;
use compact_str::{CompactString, ToCompactString};
use nix_bindings::prelude::*;
use smallvec::{SmallVec, smallvec};

use crate::resolve_build_graph::WorkspaceResolve;
use crate::vendor_deps::SourceId;

/// A map from the package name of a given dependency to its renaming spec.
pub(crate) type DependencyRenames = HashMap<CompactString, DependencyRename>;

pub(crate) struct BuildGraph {
    /// A vector storing all the nodes in the build graph, with edges
    /// represented by indices of other nodes in the same vector.
    ///
    /// Because of the way it's constructed, if package `T` depends on package
    /// `U`, then `U`'s index is guaranteed to be smaller than `T`s. Note
    /// however that the opposite is not true, i.e. just because `T` precedes
    /// `U` doesn't necessarily mean that `U` depends on it. It follows that
    /// the root of the build graph is always the last node in the vector.
    ///
    /// Each node maps 1:1 to a specific Cargo
    /// [`Package`](cargo::core::Package).
    pub(crate) nodes: Vec<BuildGraphNode>,

    /// TODO: docs.
    pub(crate) edges: Vec<NodeEdges>,

    /// Map from a package's ID to the index in the [`nodes`](Self::nodes)
    /// vector poining to the corresponding [`BuildGraphNode`].
    pkg_id_to_idx: HashMap<PackageId, usize>,
}

/// A single node in the [`BuildGraph`].
#[derive(nix_bindings::Attrset)]
#[attrset(rename_all = camelCase)]
pub(crate) struct BuildGraphNode {
    /// TODO: docs.
    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) binaries: Vec<BinaryCrate>,

    /// TODO: docs.
    #[attrset(skip_if = Option::is_none)]
    pub(crate) build_script: Option<BuildScript>,

    /// TODO: docs.
    #[attrset(skip_if = DependencyRenames::is_empty)]
    pub(crate) dependency_renames: DependencyRenames,

    /// TODO: docs.
    #[attrset(skip_if = Option::is_none)]
    pub(crate) library: Option<LibraryCrate>,

    /// TODO: docs.
    pub(crate) package_attrs: PackageAttrs,

    /// TODO: docs.
    pub(crate) package_src: Option<PathBuf>,
}

/// Edges from a node to its dependencies in the build graph.
#[derive(Default, nix_bindings::Attrset)]
#[attrset(rename_all = camelCase)]
pub(crate) struct NodeEdges {
    /// The indices of the node's dependencies in the build graph.
    pub(crate) dependencies: Vec<usize>,

    /// The indices of the node's build script dependencies in the build graph.
    pub(crate) build_dependencies: Vec<usize>,
}

#[derive(nix_bindings::Attrset, Clone)]
#[attrset(rename_all = camelCase)]
pub(crate) struct BuildScript {
    pub(crate) build_opts: BuildOpts,

    /// TODO: docs.
    pub(crate) dependency_renames: DependencyRenames,

    /// The relative path from the package root to the build script (usually
    /// "build.rs").
    pub(crate) path: CompactString,
}

/// TODO: docs.
#[derive(nix_bindings::Attrset, Clone)]
#[attrset(rename_all = camelCase)]
pub(crate) struct BuildOpts {
    pub(crate) codegen_units: Option<u32>,
    pub(crate) extra_rustc_args: Vec<CompactString>,
}

#[derive(nix_bindings::Attrset)]
#[attrset(rename_all = camelCase)]
pub(crate) struct BinaryCrate {
    pub(crate) build_opts: BuildOpts,
    pub(crate) name: CompactString,
    pub(crate) path: CompactString,
    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) required_features: Vec<CompactString>,
}

#[derive(nix_bindings::Attrset)]
#[attrset(rename_all = camelCase)]
pub(crate) struct LibraryCrate {
    pub(crate) build_opts: BuildOpts,

    /// The name of the library target. This is usually the
    /// [`package_name`](BuildNodeInfos::package_name) with dashes replaced by
    /// underscores.
    pub(crate) name: CompactString,

    /// The path to the entrypoint of the library's module tree from the root
    /// of the package, (usually `src/lib.rs`).
    pub(crate) path: CompactString,

    /// The library formats to generate when building this crate.
    #[attrset(skip_if = SmallVec::is_empty)]
    pub(crate) formats: SmallVec<[LibraryFormat; 1]>,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum LibraryFormat {
    Cdylib,
    Dylib,
    Lib,
    ProcMacro,
    Rlib,
    Staticlib,
}

#[derive(nix_bindings::Attrset, Clone)]
#[attrset(rename_all = camelCase)]
pub(crate) struct PackageAttrs {
    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) authors: Vec<String>,
    #[attrset(skip_if = Option::is_none)]
    pub(crate) description: Option<String>,
    #[attrset(with_value = |&ed| edition_as_str(ed))]
    pub(crate) edition: Edition,
    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) features: Vec<CompactString>,
    #[attrset(skip_if = Option::is_none)]
    pub(crate) homepage: Option<String>,
    #[attrset(skip_if = Option::is_none)]
    pub(crate) license: Option<CompactString>,
    #[attrset(skip_if = Option::is_none)]
    pub(crate) license_file: Option<CompactString>,
    #[attrset(skip_if = Option::is_none)]
    pub(crate) links: Option<CompactString>,
    pub(crate) name: CompactString,
    #[attrset(skip_if = Option::is_none)]
    pub(crate) readme: Option<CompactString>,
    #[attrset(skip_if = Option::is_none)]
    pub(crate) repository: Option<CompactString>,
    #[attrset(skip_if = Option::is_none)]
    pub(crate) rust_version: Option<CompactString>,
    #[attrset(with_value = ToCompactString::to_compact_string)]
    pub(crate) version: semver::Version,
}

#[derive(nix_bindings::Value, Clone)]
pub(crate) enum DependencyRename {
    Simple(CompactString),
    Extended(SmallVec<[RenameWithVersion; 2]>),
}

/// Represents a version-specific rename for the extended crateRenames format.
#[derive(nix_bindings::Attrset, Clone)]
pub(crate) struct RenameWithVersion {
    pub(crate) rename: CompactString,

    #[attrset(with_value = ToCompactString::to_compact_string)]
    pub(crate) version_req: OptVersionReq,
}

impl BuildGraph {
    pub(crate) fn new(
        root_package_id: PackageId,
        resolve: &WorkspaceResolve,
    ) -> Self {
        let mut this = Self::empty();
        this.insert_package(root_package_id, resolve);
        this
    }

    /// Inserts the package with the given ID (and all its dependencies,
    /// recursively) into the build graph, returning the index of the
    /// corresponding node.
    fn insert_package(
        &mut self,
        pkg_id: PackageId,
        resolve: &WorkspaceResolve,
    ) -> usize {
        // Return early if we've already inserted this package.
        if let Some(&node_idx) = self.pkg_id_to_idx.get(&pkg_id) {
            return node_idx;
        }

        let package =
            resolve.package(pkg_id).expect("package ID not found in workspace");

        let package_src = package
            .package_id()
            .source_id()
            .is_path()
            .then(|| package.root().to_owned());

        let mut edges = NodeEdges::default();

        for (dep_pkg_id, dep) in resolve.deps(pkg_id) {
            match dep.kind() {
                DepKind::Normal => {
                    let node_idx = self.insert_package(dep_pkg_id, resolve);
                    edges.dependencies.push(node_idx);
                },
                DepKind::Build => {
                    let node_idx = self.insert_package(dep_pkg_id, resolve);
                    edges.build_dependencies.push(node_idx);
                },
                DepKind::Development => {},
            }
        }

        let binaries = BinaryCrate::new(package, resolve)
            .map(Iterator::collect)
            .unwrap_or_default();

        let node = BuildGraphNode {
            binaries,
            build_script: BuildScript::new(package, resolve),
            dependency_renames: dependency_renames::<true>(pkg_id, resolve),
            library: LibraryCrate::new(package, resolve),
            package_attrs: PackageAttrs::new(package, resolve),
            package_src,
        };

        let node_idx = self.nodes.len();

        self.nodes.push(node);
        self.edges.push(edges);
        self.pkg_id_to_idx.insert(pkg_id, node_idx);

        node_idx
    }

    /// Returns a new, empty build graph.
    ///
    /// Note that we don't provide a `Default` impl for `BuildGraph` because a
    /// valid `BuildGraph` must always have at least one node (the root).
    fn empty() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            pkg_id_to_idx: HashMap::new(),
        }
    }
}

impl BuildGraphNode {
    pub(crate) fn source_id(&self) -> SourceId<'_> {
        self.package_attrs.source_id()
    }
}

impl LibraryCrate {
    pub(crate) fn is_proc_macro(&self) -> bool {
        &*self.formats == &[LibraryFormat::ProcMacro]
    }
}

impl LibraryFormat {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            LibraryFormat::Cdylib => "cdylib",
            LibraryFormat::Dylib => "dylib",
            LibraryFormat::Lib => "lib",
            LibraryFormat::ProcMacro => "proc-macro",
            LibraryFormat::Rlib => "rlib",
            LibraryFormat::Staticlib => "staticlib",
        }
    }
}

impl BinaryCrate {
    /// Returns an iterator over all the binary crates in the given package, or
    /// `None` if the package is not the root of the build graph.
    fn new(
        package: &Package,
        resolve: &WorkspaceResolve,
    ) -> Option<impl Iterator<Item = Self>> {
        let package_id = package.package_id();

        if &package_id != resolve.root_id() {
            return None;
        }

        let bin_targets = package.targets().iter().filter_map(|target| {
            match target.src_path() {
                TargetSourcePath::Path(src_path) => {
                    target.is_bin().then(|| (target, &**src_path))
                },
                TargetSourcePath::Metabuild => None,
            }
        });

        Some(bin_targets.map(move |(target, src_path)| {
            let path = src_path
                .strip_prefix(package.root())
                .expect("binary path is under package root")
                .display()
                .to_compact_string();

            let required_features = target
                .required_features()
                .iter()
                .flat_map(|feats| feats.iter())
                .map(|feat| (**feat).into())
                .collect();

            Self {
                build_opts: BuildOpts::new(package_id, false, resolve),
                name: target.name().into(),
                path,
                required_features,
            }
        }))
    }
}

impl BuildScript {
    /// Returns the build script of the given package, if any.
    fn new(package: &Package, resolve: &WorkspaceResolve) -> Option<Self> {
        let Some(build_script) = package
            .manifest()
            .original_toml()
            .package()
            .and_then(|pkg| pkg.build.as_ref())
        else {
            return None;
        };

        let path = match build_script {
            TomlPackageBuild::Auto(true) => "build.rs",
            TomlPackageBuild::Auto(false) => return None,
            TomlPackageBuild::SingleScript(path) => &**path,
            TomlPackageBuild::MultipleScript(_) => {
                panic!("multiple build scripts are not yet supported")
            },
        };

        let package_id = package.package_id();

        Some(Self {
            build_opts: BuildOpts::new(package_id, true, resolve),
            dependency_renames: dependency_renames::<false>(
                package_id, resolve,
            ),
            path: path.into(),
        })
    }
}

impl BuildOpts {
    fn new(
        package_id: PackageId,
        is_build_script_or_proc_macro: bool,
        resolve: &WorkspaceResolve,
    ) -> Self {
        let unit_for = if is_build_script_or_proc_macro {
            UnitFor::new_host(true, CompileKind::Host)
        } else {
            UnitFor::new_normal(resolve.compile_kind())
        };

        let compile_kind = if is_build_script_or_proc_macro {
            CompileKind::Host
        } else {
            resolve.compile_kind()
        };

        let profile = resolve.profiles().get_profile(
            package_id,
            resolve.workspace().is_member_id(package_id),
            package_id.source_id().is_path(),
            unit_for,
            compile_kind,
        );

        let extra_rustc_args = profile
            .rustflags
            .iter()
            .map(|s| s.as_str())
            .chain(
                resolve
                    .target_data()
                    .get_info(compile_kind)
                    .map_or(&[][..], |info| &*info.rustflags)
                    .iter()
                    .map(|s| &**s),
            )
            .map(Into::into)
            .collect();

        Self { codegen_units: profile.codegen_units, extra_rustc_args }
    }
}

impl LibraryCrate {
    /// Returns the library crate of the given package, if any.
    fn new(package: &Package, resolve: &WorkspaceResolve) -> Option<Self> {
        let (lib_target, crate_types) =
            package.targets().iter().find_map(|target| {
                match target.kind() {
                    TargetKind::Lib(crate_types) => {
                        Some((target, &**crate_types))
                    },
                    _ => None,
                }
            })?;

        let lib_path = match lib_target.src_path() {
            TargetSourcePath::Path(src_path) => src_path
                .strip_prefix(package.root())
                .expect("library path is under package root")
                .display()
                .to_compact_string(),

            TargetSourcePath::Metabuild => {
                panic!("library target cannot have a metabuild source path")
            },
        };

        let mut is_proc_macro = false;

        let lib_formats = crate_types
            .iter()
            .map(|crate_type| match crate_type {
                CrateType::Lib => LibraryFormat::Lib,
                CrateType::Rlib => LibraryFormat::Rlib,
                CrateType::Dylib => LibraryFormat::Dylib,
                CrateType::Cdylib => LibraryFormat::Cdylib,
                CrateType::Staticlib => LibraryFormat::Staticlib,
                CrateType::ProcMacro => {
                    is_proc_macro = true;
                    LibraryFormat::ProcMacro
                },
                other => unreachable!("{other:?} is not a library crate type"),
            })
            .collect();

        Some(Self {
            build_opts: BuildOpts::new(
                package.package_id(),
                is_proc_macro,
                resolve,
            ),
            name: lib_target.name().into(),
            path: lib_path,
            formats: lib_formats,
        })
    }
}

impl PackageAttrs {
    pub(crate) fn source_id(&self) -> SourceId<'_> {
        SourceId {
            package_name: &self.name,
            version: Cow::Owned(self.version.to_string()),
        }
    }

    fn new(package: &Package, resolve: &WorkspaceResolve) -> Self {
        let manifest = package.manifest();
        let metadata = manifest.metadata();

        Self {
            authors: metadata.authors.clone(),
            description: metadata.description.clone(),
            edition: manifest.edition(),
            features: resolve
                .features(package.package_id())
                .map(Into::into)
                .collect(),
            homepage: metadata.homepage.clone(),
            license: metadata.license.as_deref().map(Into::into),
            license_file: metadata.license_file.as_deref().map(Into::into),
            links: metadata.links.as_deref().map(Into::into),
            name: package.name().as_str().into(),
            readme: metadata.readme.as_deref().map(Into::into),
            repository: metadata.repository.as_deref().map(Into::into),
            rust_version: metadata
                .rust_version
                .as_ref()
                .map(|v| v.to_compact_string()),
            version: package.version().clone(),
        }
    }
}

/// Constructs the [`DependencyRenames`] for the package with the given ID.
///
/// The `IS_NORMAL` constant should be `true` if the renames should only include
/// [normal](DepKind::Normal) dependencies, and `false` if they should only
/// include [build](DepKind::Build) dependencies.
#[inline]
pub(crate) fn dependency_renames<const IS_NORMAL: bool>(
    package_id: PackageId,
    resolve: &WorkspaceResolve,
) -> DependencyRenames {
    let mut renames = DependencyRenames::default();

    for (_dep_pkg_id, dep) in resolve.deps(package_id) {
        match dep.kind() {
            DepKind::Normal if !IS_NORMAL => continue,
            DepKind::Build if IS_NORMAL => continue,
            DepKind::Development => continue,
            _ => {},
        }

        let Some(name_in_toml) = dep.explicit_name_in_toml() else {
            continue;
        };

        let rename_with_version = RenameWithVersion {
            rename: CompactString::const_new(name_in_toml.as_str()),
            version_req: dep.version_req().clone(),
        };

        match renames.entry(dep.package_name().as_str().into()) {
            hash_map::Entry::Occupied(mut entry) => {
                let DependencyRename::Extended(versions) = entry.get_mut()
                else {
                    unreachable!(
                        "we only create extended renames on the first pass"
                    );
                };
                versions.push(rename_with_version);
            },
            hash_map::Entry::Vacant(entry) => {
                entry.insert(DependencyRename::Extended(smallvec![
                    rename_with_version
                ]));
            },
        }
    }

    // Turn `Extended` renames with only one version back into `Simple` renames.
    renames.values_mut().for_each(|rename| {
        let DependencyRename::Extended(versions) = rename else { return };
        if versions.len() != 1 {
            return;
        }
        let name_in_toml = mem::take(versions)
            .into_iter()
            .next()
            .expect("checked length")
            .rename;
        *rename = DependencyRename::Simple(name_in_toml);
    });

    renames
}

#[inline]
pub(crate) fn edition_as_str(edition: Edition) -> &'static str {
    match edition {
        Edition::Edition2015 => "2015",
        Edition::Edition2018 => "2018",
        Edition::Edition2021 => "2021",
        Edition::Edition2024 => "2024",
        Edition::EditionFuture => "future",
    }
}

impl IntoValue for BuildGraph {
    fn into_value(self, _: &mut Context) -> impl Value + use<> {
        let mut nodes = Vec::with_capacity(self.nodes.len());

        for (node_idx, node) in self.nodes.into_iter().enumerate() {
            let dependencies = self.edges[node_idx].dependencies.clone();

            // Add a `dependencies` attribute to the build script if it has any.
            let build_script = node.build_script.clone().map(|script| {
                let dependencies =
                    self.edges[node_idx].build_dependencies.clone();

                script.merge(
                    (!dependencies.is_empty())
                        .then(|| attrset! { dependencies }),
                )
            });

            let node = node
                .merge(
                    // Add a `dependencies` attribute if the node has any.
                    (!dependencies.is_empty())
                        .then(|| attrset! { dependencies }),
                )
                .merge(attrset! { buildScript: build_script });

            nodes.push(node);
        }

        nodes
    }
}

impl ToValue for LibraryFormat {
    fn to_value(&self, _: &mut Context) -> impl Value + use<> {
        self.as_str()
    }
}
