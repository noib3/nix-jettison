use std::path::Path;

use cargo::core::Package;
use cargo::core::compiler::CrateType;
use compact_str::CompactString;
use either::Either;
use nix_bindings::prelude::*;
use semver::Version;

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
#[attrset(rename_all = "camelCase")]
pub(crate) struct RequiredBuildCrateArgs<'src> {
    pub(crate) crate_name: CompactString,
    #[attrset(with_value = |this| this.src_value())]
    pub(crate) src: CrateSource<'src>,
    pub(crate) version: Version,
}

/// The path to a crate's source directory.
pub(crate) enum CrateSource<'src> {
    /// The crate is a 3rd-party dependency which has been vendored under the
    /// given directory.
    Vendored { vendor_dir: &'src VendorDir },

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
#[derive(cauchy::Default, nix_bindings::Attrset)]
#[attrset(rename_all = "camelCase")]
pub(crate) struct OptionalBuildCrateArgs<Dep> {
    pub(crate) inner: OptionalBuildCrateArgsInner,
    pub(crate) dependencies: Dependencies<Dep>,
}

#[derive(Default, nix_bindings::Attrset)]
#[attrset(rename_all = "camelCase")]
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
    pub(crate) crate_bin: Option<()>,

    /// This is derived state from the dependencies section of the Cargo.toml
    /// of the crate.
    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) crate_renames: Vec<()>,

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
    pub(crate) r#type: Vec<CrateType>,
}

#[derive(cauchy::Default, nix_bindings::Attrset)]
#[attrset(rename_all = "camelCase")]
pub(crate) struct Dependencies<Dep> {
    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) normal: Vec<Dep>,

    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) build: Vec<Dep>,
}

/// Unlike [`RequiredBuildCrateArgs`] and [`OptionalBuildCrateArgs`], these
/// arguments don't depend on the particular crate being built.
#[derive(Default, nix_bindings::Attrset)]
#[attrset(rename_all = "camelCase", skip_if = Option::is_none)]
pub(crate) struct GlobalBuildCrateArgs<'a> {
    pub(crate) build_tests: Option<bool>,
    pub(crate) cargo: Option<NixDerivation<'a>>,
    pub(crate) crate_overrides: Option<NixAttrset<'a>>,
    pub(crate) release: Option<bool>,
    pub(crate) rust: Option<NixDerivation<'a>>,
}

impl RequiredBuildCrateArgs<'_> {
    fn src_value(&self) -> impl Value {
        match &self.src {
            CrateSource::Vendored { vendor_dir } => Either::Left(
                vendor_dir
                    .get_package_src(self.crate_name.as_str(), &self.version),
            ),

            CrateSource::Workspace { workspace_root, path_in_workspace } => {
                Either::Right(workspace_root.join(path_in_workspace.as_str()))
            },
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
        Dep: Value,
    {
        // SAFETY: 'inner' and 'dependencies' don't have any overlapping keys.
        unsafe { self.inner.borrow().concat(self.dependencies.to_attrset()) }
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

    fn to_attrset(&self) -> impl Attrset
    where
        Dep: Value,
    {
        attrset! {
            dependencies: &*self.normal,
            buildDependencies: &*self.build,
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

impl From<&Package> for RequiredBuildCrateArgs<'_> {
    fn from(_package: &Package) -> Self {
        todo!();
    }
}

impl From<&Package> for OptionalBuildCrateArgsInner {
    fn from(_package: &Package) -> Self {
        todo!();
    }
}
