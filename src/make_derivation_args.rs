use core::fmt::Write;
use core::iter;
use std::borrow::Cow;
use std::ffi::CString;

use cargo::core::compiler::CompileTarget;
use compact_str::{CompactString, ToCompactString};
use either::Either;
use indoc::formatdoc;
use nix_bindings::prelude::*;

use crate::build_graph::{
    BinaryCrate,
    BuildGraphNode,
    BuildScript,
    LibraryCrate,
};
use crate::build_node_attrs::{CrateType, NodeType};
use crate::build_package::BuildPackageArgs;

/// All the arguments needed to create the attribute set given to
/// `stdenv.mkDerivation` to build a single node in the build graph.
pub(crate) struct MakeDerivationArgs<'args, Deps, Src> {
    /// The derivation for the package's build script, if any.
    pub(crate) build_script_drv: Option<NixDerivation<'args>>,

    /// The
    /// [`BuildPackageArgs::crate_overrides`](crate::build_package::BuildPackageArgs::crate_overrides) field.
    pub(crate) crate_overrides: Option<NixAttrset<'args>>,

    /// The list of dependencies needed to build the node.
    ///
    /// This must be an iterator over derivations created with `mkDerivation
    /// args`, where `args` was an instance of `Self`.
    pub(crate) dependencies: Deps,

    /// The
    /// [`BuildPackageArgs::global_overrides`](crate::build_package::BuildPackageArgs::global_overrides) field.
    pub(crate) global_overrides: Option<NixAttrset<'args>>,

    /// The node's build attributes coming from the build graph resolution step.
    pub(crate) node: &'args BuildNodeAttrs,

    /// The derivation for the `parse-build-script-output` shell script.
    pub(crate) parse_build_script_output: NixDerivation<'args>,

    /// Whether the node should be built in release mode.
    pub(crate) release: bool,

    /// The `rustc` derivation to include in the derivation's `buildInputs`.
    pub(crate) rustc: NixDerivation<'args>,

    /// The derivation pointing to the node's source code.
    pub(crate) src: Src,

    /// A handle to Nixpkgs's standard build environment.
    pub(crate) stdenv: NixAttrset<'args>,

    /// The compilation `--target` to pass to `rustc`, if any.
    ///
    /// This should only be set when cross-compiling.
    pub(crate) target: Option<CompileTarget>,
}

#[expect(clippy::too_many_arguments)]
pub(crate) fn make_derivation<'dep, Deps>(
    _type: DerivationType,
    _node: &BuildGraphNode,
    _src: impl Value,
    _deps: NixDerivation,
    _direct_deps: Deps,
    _args: &MakeDerivationGlobalArgs,
    _ctx: &mut Context,
) -> Result<NixDerivation<'static>>
where
    Deps: Iterator<Item = (&'dep BuildGraphNode, NixDerivation<'dep>)> + Clone,
{
    todo!();
}

pub(crate) enum DerivationType<'graph> {
    /// TODO: docs.
    BuildScript(&'graph BuildScript),

    /// TODO: docs.
    Library {
        build_script: Option<NixDerivation<'static>>,
        library: &'graph LibraryCrate,
    },

    /// TODO: docs.
    Binaries {
        build_script: Option<NixDerivation<'static>>,
        library: Option<NixDerivation<'static>>,
        binaries: &'graph [BinaryCrate],
    },
}

#[derive(Clone)]
pub(crate) struct MakeDerivationGlobalArgs<'args> {
    /// The
    /// [`BuildPackageArgs::crate_overrides`](crate::build_package::BuildPackageArgs::crate_overrides) field.
    pub(crate) crate_overrides: Option<NixAttrset<'args>>,

    /// The
    /// [`BuildPackageArgs::global_overrides`](crate::build_package::BuildPackageArgs::global_overrides) field.
    pub(crate) global_overrides: Option<NixAttrset<'args>>,

    /// The node's build attributes coming from the build graph resolution step.
    pub(crate) mk_derivation: NixLambda<'args>,

    /// The derivation for the `parse-build-script-output` shell script.
    pub(crate) parse_build_script_output: NixDerivation<'args>,

    /// Whether the node should be built in release mode.
    pub(crate) release: bool,

    /// The `rustc` derivation to include in the derivation's `buildInputs`.
    pub(crate) rustc: NixDerivation<'args>,

    /// A handle to Nixpkgs's standard build environment.
    pub(crate) stdenv: NixAttrset<'args>,

    /// The compilation `--target` to pass to `rustc`, if any.
    ///
    /// This should only be set when cross-compiling.
    pub(crate) target: Option<CompileTarget>,
}

impl<'args> MakeDerivationGlobalArgs<'args> {
    pub(crate) fn new(
        args: &BuildPackageArgs<'args>,
        ctx: &mut Context,
    ) -> Result<Self> {
        let stdenv = args.pkgs.get::<NixAttrset>(c"stdenv", ctx)?;

        let write_shell_script_bin =
            args.pkgs.get::<NixLambda>(c"writeShellScriptBin", ctx)?;

        let parse_build_script_output = write_shell_script_bin
            .call_multi(
                (
                    c"parse-build-script-output",
                    include_str!("./parse-build-script-output.sh"),
                ),
                ctx,
            )?
            .force_into::<NixDerivation>(ctx)?;

        let rustc = match args.rustc {
            Some(rustc) => rustc,
            None => args.pkgs.get::<NixDerivation>(c"rustc", ctx)?,
        };

        Ok(Self {
            crate_overrides: args.crate_overrides,
            global_overrides: args.global_overrides,
            mk_derivation: stdenv.get::<NixLambda>(c"mkDerivation", ctx)?,
            parse_build_script_output,
            release: args.release,
            rustc,
            stdenv,
            target: None,
        })
    }
}

impl<'a, Src, Deps> MakeDerivationArgs<'a, Deps, Src>
where
    Src: Value,
    Deps: Iterator<Item = (&'a BuildNodeAttrs, NixDerivation<'a>)> + Clone,
{
    /// Converts `self` into the final attribute set given to
    /// `stdenv.mkDerivation`.
    pub(crate) fn into_attrs(
        self,
        ctx: &mut Context,
    ) -> Result<impl Attrset + Value + use<'a, Src, Deps>> {
        let build_inputs = self
            .build_script_drv
            .clone()
            .into_iter()
            .chain(self.dependencies.clone().map(|(_, drv)| drv))
            .collect::<Vec<_>>();

        let base_args = attrset! {
            name: self.node.derivation_name(),
            buildInputs: build_inputs,
            nativeBuildInputs: [self.parse_build_script_output, self.rustc],
            configurePhase: self.configure_phase(ctx)?,
            buildPhase: self.build_phase(ctx)?,
            installPhase: self.install_phase(ctx)?,
            dontStrip: true,
            // See https://github.com/NixOS/nixpkgs/issues/218712.
            stripExclude: [ c"*.rlib" ],
            version: self.node.version.to_compact_string(),
            src: self.src,
        };

        let args = base_args.merge(self.global_overrides);

        let Some(crate_overrides) = self.crate_overrides else {
            return Ok(Either::Left(args));
        };

        let package_name_cstr = CString::new(&*self.node.package_name)
            .expect("package name doesn't contain NUL bytes");

        let Some(override_fun) =
            crate_overrides.get_opt::<NixLambda>(&*package_name_cstr, ctx)?
        else {
            return Ok(Either::Left(args));
        };

        let overrides = override_fun
            .call(Value::borrow(&args), ctx)?
            .force_into::<NixAttrset>(ctx)?;

        Ok(Either::Right(args.merge(overrides)))
    }

    #[expect(clippy::too_many_lines)]
    fn configure_phase(&self, ctx: &mut Context) -> Result<String> {
        // ## Native libraries
        // 1: get the list of native libraries from somewhere (I'm assuming the
        //    native_build_inputs?);
        // 2: for every native library, add a `-C
        //    link-arg={full_path_to_{*.so|*.dylib|*.a}}` argument to the flags
        //    given to `rustc`
        // 3: there may be other linker flags coming from build scripts, but
        //    those should be taken care of by the program that parses its
        //    output, so that the only thing we might have to do is pass
        //    `$EXTRA_RUSTC_ARGS` to the `rustc` calls;
        let host_platform =
            self.stdenv.get::<NixAttrset>(c"hostPlatform", ctx)?;

        let cpu_platform = host_platform
            .get::<NixAttrset>(c"parsed", ctx)?
            .get::<NixAttrset>(c"cpu", ctx)?;

        let rust_platform = host_platform
            .get::<NixAttrset>(c"rust", ctx)?
            .get::<NixAttrset>(c"platform", ctx)?;

        let is_cpu_little_endian = cpu_platform
            .get::<NixAttrset>(c"significantByte", ctx)?
            .get::<CompactString>(c"name", ctx)?
            == "littleEndian";

        let target_arch = rust_platform.get::<CompactString>(c"arch", ctx)?;
        let target_endian = if is_cpu_little_endian { "little" } else { "big" };
        let target_os = rust_platform.get::<CompactString>(c"os", ctx)?;
        let target_pointer_width =
            if host_platform.get::<bool>(c"isILP32", ctx)? {
                32
            } else {
                cpu_platform.get::<u8>(c"bits", ctx)?
            };
        let target_vendor = host_platform
            .get::<NixAttrset>(c"parsed", ctx)?
            .get::<NixAttrset>(c"vendor", ctx)?
            .get::<CompactString>(c"name", ctx)?;

        let manifest_links = self.node.links.as_deref().unwrap_or("");

        let pkg_authors =
            self.node.authors.iter().fold(String::new(), |mut acc, author| {
                if !acc.is_empty() {
                    acc.push(':');
                }
                acc.push_str(author);
                acc
            });
        let pkg_description = self
            .node
            .description
            .as_deref()
            .map_or(Cow::Borrowed(""), |desc| {
                shell_escape::escape(desc.into())
            });
        let pkg_homepage = self.node.homepage.as_deref().unwrap_or("");
        let pkg_license = self.node.license.as_deref().unwrap_or("");
        let pkg_license_file = self.node.license_file.as_deref().unwrap_or("");
        let pkg_name = &*self.node.package_name;
        let pkg_readme = self.node.readme.as_deref().unwrap_or("");
        let pkg_repository = self.node.repository.as_deref().unwrap_or("");
        let pkg_rust_version = self.node.rust_version.as_deref().unwrap_or("");
        let pkg_version = &*self.node.version.to_compact_string();
        let pkg_version_major = self.node.version.major;
        let pkg_version_minor = self.node.version.minor;
        let pkg_version_patch = self.node.version.patch;
        let pkg_version_pre = self.node.version.pre.as_str();

        let debug = if self.release { "1" } else { "" };
        let host = self
            .stdenv
            .get::<NixAttrset>(c"buildPlatform", ctx)?
            .get::<NixAttrset>(c"rust", ctx)?
            .get::<CompactString>(c"rustcTargetSpec", ctx)?;
        let opt_level = if self.release { 3 } else { 0 };
        let profile = if self.release { "release" } else { "debug" };
        let target = host_platform
            .get::<NixAttrset>(c"rust", ctx)?
            .get::<CompactString>(c"rustcTargetSpec", ctx)?;

        let mut configure_phase = formatdoc!(
            r#"
                runHook preConfigure
                export CARGO_CFG_TARGET_ARCH={target_arch}
                export CARGO_CFG_TARGET_ENDIAN={target_endian}
                export CARGO_CFG_TARGET_ENV="gnu"
                export CARGO_CFG_TARGET_FAMILY="unix"
                export CARGO_CFG_TARGET_OS={target_os}
                export CARGO_CFG_TARGET_POINTER_WIDTH={target_pointer_width}
                export CARGO_CFG_TARGET_VENDOR={target_vendor}
                export CARGO_CFG_UNIX=1
                export CARGO_MANIFEST_DIR=$(pwd)
                export CARGO_MANIFEST_LINKS={manifest_links}
                export CARGO_PKG_AUTHORS="{pkg_authors}"
                export CARGO_PKG_DESCRIPTION={pkg_description}
                export CARGO_PKG_HOMEPAGE="{pkg_homepage}"
                export CARGO_PKG_LICENSE="{pkg_license}"
                export CARGO_PKG_LICENSE_FILE="{pkg_license_file}"
                export CARGO_PKG_NAME={pkg_name}
                export CARGO_PKG_README="{pkg_readme}"
                export CARGO_PKG_REPOSITORY="{pkg_repository}"
                export CARGO_PKG_RUST_VERSION="{pkg_rust_version}"
                export CARGO_PKG_VERSION={pkg_version}
                export CARGO_PKG_VERSION_MAJOR={pkg_version_major}
                export CARGO_PKG_VERSION_MINOR={pkg_version_minor}
                export CARGO_PKG_VERSION_PATCH={pkg_version_patch}
                export CARGO_PKG_VERSION_PRE="{pkg_version_pre}"
                export DEBUG={debug}
                export HOST={host}
                export NUM_JOBS=$NIX_BUILD_CORES
                export OPT_LEVEL={opt_level}
                export PROFILE={profile}
                export RUSTC="rustc"
                export RUSTDOC="rustdoc"
                export TARGET={target}
            "#
        );

        if let Some(build_drv) = &self.build_script_drv {
            let out_path = build_drv.out_path(ctx)?;

            writeln!(
                &mut configure_phase,
                "export OUT_DIR={}/out",
                out_path.display()
            )
            .expect("writing to string can't fail");

            let shell_script_to_source = match self.node.r#type {
                NodeType::Bin(_) => "bin.sh",
                NodeType::Lib(_) => "lib.sh",
                NodeType::BuildScript(_) => unreachable!(
                    "build scripts can't depend on other build scripts"
                ),
            };

            writeln!(
                &mut configure_phase,
                "source {}/{shell_script_to_source}",
                out_path.display(),
            )
            .expect("writing to string can't fail");
        }

        configure_phase.push_str("runHook postConfigure");

        Ok(configure_phase)
    }

    fn build_phase(&self, ctx: &mut Context) -> Result<String> {
        let crate_types = match &self.node.r#type {
            NodeType::Bin(bins) => {
                Either::Right(bins.iter().map(CrateType::Bin))
            },
            NodeType::Lib(lib) => {
                Either::Left(iter::once(CrateType::Lib(&lib)))
            },
            NodeType::BuildScript(path) => {
                Either::Left(iter::once(CrateType::BuildScript(path)))
            },
        };

        let mut build_phase = "runHook preBuild".to_owned();

        for crate_type in crate_types {
            build_phase.push_str("\nrustc");

            for rustc_arg in self.node.build_rustc_args(
                self.release,
                crate_type,
                self.dependencies.clone(),
                self.target.as_ref(),
                ctx,
            ) {
                build_phase.push(' ');
                build_phase.push_str(rustc_arg.as_ref());
            }

            // Append any extra arguments coming from build scripts.
            build_phase.push_str(" ${EXTRA_RUSTC_ARGS:-}");
        }

        build_phase.push_str("\nrunHook postBuild");

        Ok(build_phase)
    }

    fn install_phase(&self, _ctx: &mut Context) -> Result<String> {
        let mut install_phase = formatdoc!(
            "
                runHook preInstall
                mkdir -p $out
                cp -r {out_dir}/* $out
            ",
            out_dir = self.node.out_dir(),
        );

        if self.node.r#type.is_build_script() {
            for feature in &self.node.features {
                let feature = feature.to_uppercase().replace('-', "_");
                install_phase.push_str("export CARGO_FEATURE_");
                install_phase.push_str(&*feature);
                install_phase.push_str("=1\n");
            }

            install_phase.push_str(&formatdoc!(
                r"
                    export OUT_DIR=$out/out
                    mkdir -p $OUT_DIR
                    $out/build_script_build | tee $out/build_script_output.txt
                    parse-build-script-output \
                        $out/build_script_output.txt \
                        $out/common.sh \
                        $out/lib.sh \
                        $out/bin.sh \
                        EXTRA_RUSTC_ARGS \
                        {package_name} \
                        {package_version}
                ",
                package_name = self.node.package_name,
                package_version = self.node.version.to_compact_string(),
            ));
        }

        install_phase.push_str("runHook postInstall\n");

        Ok(install_phase)
    }
}
