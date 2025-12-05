use std::path::Path;

use cargo::core::compiler::CrateType;
use compact_str::CompactString;
use either::Either;
use nix_bindings::prelude::{Attrset, NixAttrset, NixDerivation, Value};
use semver::Version;

use crate::vendor_deps::VendorDir;

/// The arguments accepted by [`pkgs.buildRustCrate`][buildRustCrate].
///
/// [buildRustCrate]: https://github.com/NixOS/nixpkgs/blob/d792a6e0cd4ba35c90ea787b717d72410f56dc40/pkgs/build-support/rust/build-rust-crate/default.nix
pub(crate) struct BuildCrateArgs<'global, 'src, Dep> {
    pub(crate) mandatory: MandatoryBuildCrateArgs<'src>,
    pub(crate) optional: OptionalBuildCrateArgs<Dep>,
    pub(crate) global: GlobalBuildCrateArgs<'global>,
}

/// The mandatory, crate-specific arguments accepted by `pkgs.buildRustCrate`.
#[derive(nix_bindings::Attrset)]
#[attrset(rename_all = "camelCase")]
pub(crate) struct MandatoryBuildCrateArgs<'src> {
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
    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) authors: Vec<String>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) build: Option<String>,

    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) build_dependencies: Vec<Dep>,

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

    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) dependencies: Vec<Dep>,

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

/// Unlike [`MandatoryBuildCrateArgs`] and [`OptionalBuildCrateArgs`], these
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

impl<Dep> BuildCrateArgs<'_, '_, Dep>
where
    OptionalBuildCrateArgs<Dep>: Attrset,
{
    pub(crate) fn to_attrset(&self) -> impl Attrset {
        // SAFETY: the three inner attrsets don't contain any overlapping keys.
        unsafe {
            self.mandatory
                .borrow()
                .concat(self.optional.borrow())
                .concat(self.global.borrow())
        }
    }
}

impl MandatoryBuildCrateArgs<'_> {
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

impl<'src, Dep> From<MandatoryBuildCrateArgs<'src>>
    for BuildCrateArgs<'_, 'src, Dep>
{
    fn from(mandatory: MandatoryBuildCrateArgs<'src>) -> Self {
        Self {
            mandatory,
            optional: OptionalBuildCrateArgs::default(),
            global: GlobalBuildCrateArgs::default(),
        }
    }
}
