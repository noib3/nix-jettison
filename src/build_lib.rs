use core::ops::Not;
use std::collections::HashMap;

use compact_str::{CompactString, format_compact};
use indoc::{formatdoc, indoc};
use nix_bindings::prelude::*;

#[derive(nix_bindings::Attrset)]
#[attrset(rename_all = camelCase)]
pub(crate) struct BuildLibArgs {
    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) authors: Vec<String>,

    /// The path to the build script from the root of the crate. `None` if
    /// there's no build script or if its path is just the default `build.rs`.
    #[attrset(skip_if = Option::is_none)]
    pub(crate) build: Option<CompactString>,

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

    #[attrset(skip_if = Option::is_none)]
    pub(crate) edition: Option<CompactString>,

    /// This is derived state from the Cargo config.
    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) extra_rustc_opts: Vec<CompactString>,

    #[attrset(skip_if = Vec::is_empty)]
    pub(crate) features: Vec<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) homepage: Option<CompactString>,

    /// The name of the library target, or `None` if it's the
    /// [`package_name`](Self::package_name) with dashes replaced by
    /// underscores.
    #[attrset(skip_if = Option::is_none)]
    pub(crate) lib_name: Option<CompactString>,

    /// The path to the entrypoint of the library's module tree from the root
    /// of the package, or `None` if it's the default `src/lib.rs`.
    #[attrset(skip_if = Option::is_none)]
    pub(crate) lib_path: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) license_file: Option<CompactString>,

    #[attrset(skip_if = Option::is_none)]
    pub(crate) links: Option<CompactString>,

    /// The name of the package this library is in.
    pub(crate) package_name: CompactString,

    #[attrset(skip_if = Not::not)]
    pub(crate) proc_macro: bool,

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

impl BuildLibArgs {
    pub(crate) fn to_mk_derivation_args<Src: Value, Drv: ToValue>(
        &self,
        src: Src,
        build_inputs: &[Drv],
        native_build_inputs: &[Drv],
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
            buildPhase: formatdoc!("
                runHook preBuild
                rustc {}
                runHook postBuild
            ", self.rustc_args().into_iter().fold(String::new(), |mut args, arg| {
                args.push(' ');
                args.push_str(arg.as_ref());
                args
            })),
            installPhase: indoc!("
                runHook preInstall
                mkdir -p $lib/lib
                cp -r target/lib/* $lib/lib
                runHook postInstall
            "),
            dontStrip: false,
            stripExclude: [ c"*.rlib" ].into_value(),
            outputs: [ c"out", c"lib" ].into_value(),
        }
    }

    /// Returns the list of command-line arguments to pass to `rustc` to build
    /// this library.
    fn rustc_args(&self) -> impl IntoIterator<Item = impl AsRef<str>> {
        Vec::<CompactString>::new()
    }
}
