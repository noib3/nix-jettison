use std::path::Path;

use cargo::core::{Package, Resolve};
use compact_str::{CompactString, ToCompactString};
use either::Either;
use nix_bindings::prelude::*;

use crate::resolve_build_graph::ResolveBuildGraphArgs;
use crate::vendor_deps::VendorDir;

/// The arguments accepted by [`pkgs.buildRustCrate`][buildRustCrate].
///
/// [buildRustCrate]: https://github.com/NixOS/nixpkgs/blob/d792a6e0cd4ba35c90ea787b717d72410f56dc40/pkgs/build-support/rust/build-rust-crate/default.nix
pub(crate) struct BuildCrateArgs<'global, 'src, Dep> {
    pub(crate) required: RequiredBuildCrateArgs<'src>,
    pub(crate) optional: OptionalBuildCrateArgs<Dep>,
    pub(crate) global: GlobalBuildCrateArgs<'global>,
}

/// The required, crate-specific arguments accepted by `pkgs.buildRustCrate`.
#[derive(nix_bindings::Attrset)]
#[attrset(rename_all = camelCase)]
pub(crate) struct RequiredBuildCrateArgs<'src> {
    pub(crate) crate_name: CompactString,
    #[attrset(with_value = Self::src_value)]
    pub(crate) src: CrateSource<'src>,
    pub(crate) version: CompactString,
}

/// The path to a crate's source directory.
pub(crate) enum CrateSource<'src> {
    /// The crate is a 3rd-party dependency which has been vendored under the
    /// given directory.
    Vendored { vendor_dir: &'src Path },

    /// The crate source is in the workspace of the root package being built.
    Workspace {
        /// An absolute path in the Nix store pointing to the root of the
        /// workspace (this is usually the value of the `src` argument given
        /// to `jettison.buildPackage`).
        workspace_root: &'src Path,

        /// The relative path in the workspace to the crate's source (e.g.
        /// `"crates/my_crate"`).
        path_in_workspace: CompactString,
    },
}

/// The optional, crate-specific arguments accepted by `pkgs.buildRustCrate`.
#[derive(cauchy::Default)]
pub(crate) struct OptionalBuildCrateArgs<Dep> {
    pub(crate) inner: OptionalBuildCrateArgsInner,
    pub(crate) dependencies: Dependencies<Dep>,
}

#[derive(Default, nix_bindings::Attrset)]
#[attrset(rename_all = camelCase)]
pub(crate) struct OptionalBuildCrateArgsInner {
    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) authors: Vec<String>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) build: Option<String>,

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

    #[attrset(skip_if = Option::is_none)]
    pub(crate) lib_name: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) lib_path: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) license_file: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) links: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) readme: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) repository: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) rust_version: Option<CompactString>,

    /// If set, `buildRustCrate` will set the `crateType` to the given value,
    /// otherwise it will default to `"lib"`.
    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) r#type: Vec<CompactString>,
}

#[derive(cauchy::Default, nix_bindings::Attrset)]
#[attrset(bounds = { Dep: ToValue })]
pub(crate) struct Dependencies<Dep> {
    #[attrset(rename = "dependencies", skip_if = Vec::is_empty)]
    pub(crate) normal: Vec<Dep>,

    #[attrset(rename = "buildDependencies", skip_if = Vec::is_empty)]
    pub(crate) build: Vec<Dep>,
}

/// Unlike [`RequiredBuildCrateArgs`] and [`OptionalBuildCrateArgs`], these
/// arguments don't depend on the particular crate being built.
#[derive(Default, nix_bindings::Attrset)]
#[attrset(rename_all = camelCase, skip_if = Option::is_none)]
pub(crate) struct GlobalBuildCrateArgs<'a> {
    pub(crate) build_tests: Option<bool>,
    pub(crate) cargo: Option<NixDerivation<'a>>,
    pub(crate) crate_overrides: Option<NixAttrset<'a>>,
    pub(crate) release: Option<bool>,
    pub(crate) rust: Option<NixDerivation<'a>>,
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

    fn src_value(&self) -> impl Value {
        struct WorkspacePath<'a> {
            workspace_root: &'a Path,
            path_in_workspace: &'a Path,
        }

        impl Value for WorkspacePath<'_> {
            fn kind(&self) -> ValueKind {
                ValueKind::String
            }

            unsafe fn write(
                &self,
                dest: core::ptr::NonNull<nix_bindings::sys::Value>,
                namespace: impl nix_bindings::namespace::Namespace,
                ctx: &mut Context,
            ) -> Result<()> {
                let args = attrset! {
                    path: self.workspace_root.join(self.path_in_workspace),
                    name: self
                        .path_in_workspace
                        .file_name()
                        .expect("path is not empty"),
                };

                let path = ctx
                    .builtins()
                    .path(ctx)
                    .call(args, ctx)
                    .expect(
                        "arguments are valid and builtins.path returns a \
                         string",
                    )
                    .force_into::<String>(ctx)?;

                // SAFETY: up to the caller.
                unsafe { path.write(dest, namespace, ctx) }
            }
        }

        match &self.src {
            CrateSource::Vendored { vendor_dir } => {
                Either::Left(vendor_dir.join(VendorDir::dir_name(
                    self.crate_name.as_str(),
                    &self.version,
                )))
            },

            CrateSource::Workspace { workspace_root, path_in_workspace } => {
                Either::Right(WorkspacePath {
                    workspace_root,
                    path_in_workspace: Path::new(path_in_workspace.as_str()),
                })
            },
        }
    }
}

impl<'src> CrateSource<'src> {
    fn new(package: &Package, args: &ResolveBuildGraphArgs<'src>) -> Self {
        if package.package_id().source_id().is_path() {
            let workspace_root = args.src;
            let path_in_workspace = package
                .root()
                .strip_prefix(workspace_root)
                .expect("package root is under workspace root")
                .to_str()
                .expect("workspace-relative path is valid UTF-8")
                .into();
            Self::Workspace { workspace_root, path_in_workspace }
        } else {
            Self::Vendored { vendor_dir: args.vendor_dir }
        }
    }
}

impl<Dep> OptionalBuildCrateArgs<Dep> {
    pub(crate) fn map_deps<NewDep>(
        self,
        fun: impl FnMut(Dep) -> NewDep + Clone,
    ) -> OptionalBuildCrateArgs<NewDep> {
        OptionalBuildCrateArgs {
            inner: self.inner,
            dependencies: self.dependencies.map(fun),
        }
    }

    fn to_attrset(&self) -> impl Attrset
    where
        Dep: ToValue,
    {
        // SAFETY: 'inner' and 'dependencies' don't have any overlapping keys.
        unsafe { self.inner.borrow().concat(self.dependencies.borrow()) }
    }
}

impl OptionalBuildCrateArgsInner {
    pub(crate) fn new(package: &Package, resolve: &Resolve) -> Self {
        let manifest = package.manifest();
        let metadata = manifest.metadata();
        let package_id = package.package_id();

        Self {
            authors: metadata.authors.clone(),
            build: None,
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
            lib_name: None,
            lib_path: None,
            license_file: metadata.license_file.as_deref().map(Into::into),
            links: metadata.links.as_deref().map(Into::into),
            readme: metadata.readme.as_deref().map(Into::into),
            repository: metadata.repository.as_deref().map(Into::into),
            rust_version: metadata
                .rust_version
                .as_ref()
                .map(|v| v.to_compact_string()),
            r#type: Vec::new(),
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

impl<Dep: Value> ToValue for BuildCrateArgs<'_, '_, Dep> {
    fn to_value(&self) -> impl Value {
        // SAFETY: the three inner attrsets don't contain any overlapping keys.
        unsafe {
            self.required
                .borrow()
                .concat(self.optional.to_attrset())
                .concat(self.global.borrow())
        }
    }
}
