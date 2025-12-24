use core::fmt::Write;
use core::iter;
use std::borrow::Cow;
use std::env::consts::DLL_EXTENSION;
use std::ffi::CString;

use cargo::core::Edition;
use cargo::core::compiler::CompileTarget;
use compact_str::{CompactString, ToCompactString, format_compact};
use either::Either;
use indoc::formatdoc;
use nix_bindings::prelude::*;

use crate::build_graph::{
    BinaryCrate,
    BuildGraphNode,
    BuildOpts,
    BuildScript,
    DependencyRename,
    DependencyRenames,
    LibraryCrate,
    PackageAttrs,
    PackageSource,
    RenameWithVersion,
    edition_as_str,
};
use crate::build_package::BuildPackageArgs;
use crate::vendor_deps::VendoredSources;

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
pub(crate) struct GlobalArgs<'args, 'lock, 'builtins> {
    /// The compilation `--target` to pass to `rustc`, if any.
    ///
    /// This should only be set when cross-compiling.
    pub(crate) compile_target: Option<CompileTarget>,

    /// The
    /// [`BuildPackageArgs::crate_overrides`](crate::build_package::BuildPackageArgs::crate_overrides) field.
    pub(crate) crate_overrides: Option<NixAttrset<'args>>,

    /// The
    /// [`BuildPackageArgs::global_overrides`](crate::build_package::BuildPackageArgs::global_overrides) field.
    pub(crate) global_overrides: Option<NixAttrset<'args>>,

    /// The node's build attributes coming from the build graph resolution step.
    pub(crate) mk_derivation: NixLambda<'args>,

    /// The `builtins.path` function.
    pub(crate) mk_path: NixLambda<'builtins>,

    /// The derivation for the `parse-build-script-output` shell script.
    pub(crate) parse_build_script_output: NixDerivation<'args>,

    /// Whether the node should be built in release mode.
    pub(crate) release: bool,

    /// The `rustc` derivation to include in the derivation's `buildInputs`.
    pub(crate) rustc: NixDerivation<'args>,

    /// A handle to Nixpkgs's standard build environment.
    pub(crate) stdenv: NixAttrset<'args>,

    /// TODO: docs.
    pub(crate) vendored_sources: &'args VendoredSources<'lock>,
}

struct Crate<'a> {
    path: &'a str,
    name: &'a str,
    types_arg: CompactString,
    is_proc_macro: bool,
    is_compiled_for_host: bool,
    deps_renames: &'a DependencyRenames,
    build_opts: &'a BuildOpts,
}

pub(crate) fn make_deps<'dep>(
    package: &PackageAttrs,
    _direct_deps: impl Iterator<Item = NixDerivation<'dep>>,
    mk_derivation: &NixLambda,
    ctx: &mut Context,
) -> Result<NixDerivation<'static>> {
    let args = attrset! {
        name: format_compact!("{}-{}-deps", package.name, package.version),
    };

    mk_derivation.call(args, ctx)?.force_into(ctx)
}

#[expect(clippy::too_many_arguments)]
pub(crate) fn make_derivation<'a, Deps>(
    r#type: DerivationType<'a>,
    node: &BuildGraphNode,
    deps: NixDerivation<'a>,
    direct_deps: Deps,
    args: &'a GlobalArgs,
    ctx: &mut Context,
) -> Result<NixDerivation<'static>>
where
    Deps: Iterator<Item = (&'a BuildGraphNode, NixDerivation<'a>)> + Clone,
{
    args.mk_derivation
        .call(
            make_derivation_args(r#type, node, deps, direct_deps, args, ctx)?,
            ctx,
        )?
        .force_into(ctx)
}

#[expect(clippy::too_many_arguments)]
fn make_derivation_args<'a, Deps>(
    r#type: DerivationType<'a>,
    node: &BuildGraphNode,
    deps: NixDerivation<'a>,
    direct_deps: Deps,
    args: &'a GlobalArgs,
    ctx: &mut Context,
) -> Result<impl Attrset + Value + use<'a, Deps>>
where
    Deps: Iterator<Item = (&'a BuildGraphNode, NixDerivation<'a>)> + Clone,
{
    let build_script_drv = r#type.build_script_drv();

    let build_inputs = build_script_drv
        .into_iter()
        .chain(r#type.library_drv())
        .chain(iter::once(deps.clone()))
        .chain(direct_deps.clone().map(|(_node, drv)| drv))
        .collect::<Vec<_>>();

    let derivation_name = format_compact!(
        "{}-{}-{}",
        node.package_attrs.name,
        node.package_attrs.version,
        r#type.derivation_name_suffix(),
    );

    let src = match &node.package_src {
        PackageSource::Vendored => {
            let source_id = node.package_attrs.source_id();
            args.vendored_sources.get(source_id).expect("source is vendored")
        },
        PackageSource::Path(path) => {
            let name = path.file_name().expect("not empty");
            args.mk_path.call(attrset! { path: &**path, name }, ctx)?
        },
    };

    let configure_phase = configure_phase(
        &node.package_attrs,
        build_script_drv,
        r#type.is_library(),
        args.release,
        args.stdenv,
        ctx,
    )?;

    let build_phase = build_phase(
        &r#type,
        node,
        direct_deps,
        deps,
        args.release,
        args.compile_target.as_ref(),
        ctx,
    )?;

    let install_phase =
        install_phase(&node.package_attrs, r#type.is_build_script());

    let base_args = attrset! {
        name: derivation_name,
        src,
        buildInputs: build_inputs,
        nativeBuildInputs: [args.parse_build_script_output, args.rustc],
        configurePhase: configure_phase,
        buildPhase: build_phase,
        installPhase: install_phase,
        dontStrip: true,
        // See https://github.com/NixOS/nixpkgs/issues/218712.
        stripExclude: [ c"*.rlib" ],
        version: node.package_attrs.version.to_compact_string(),
    };

    let res = base_args.merge(args.global_overrides);

    let Some(crate_overrides) = args.crate_overrides else {
        return Ok(Either::Left(res));
    };

    let package_name_cstr = CString::new(&*node.package_attrs.name)
        .expect("package name doesn't contain NUL bytes");

    let Some(override_fun) =
        crate_overrides.get_opt::<NixLambda>(&*package_name_cstr, ctx)?
    else {
        return Ok(Either::Left(res));
    };

    let overrides = override_fun
        .call(Value::borrow(&res), ctx)?
        .force_into::<NixAttrset>(ctx)?;

    Ok(Either::Right(res.merge(overrides)))
}

#[expect(clippy::too_many_arguments)]
fn build_phase<'dep, Deps>(
    r#type: &DerivationType,
    node: &BuildGraphNode,
    direct_deps: Deps,
    deps: NixDerivation,
    is_release: bool,
    target: Option<&CompileTarget>,
    ctx: &mut Context,
) -> Result<String>
where
    Deps: Iterator<Item = (&'dep BuildGraphNode, NixDerivation<'dep>)> + Clone,
{
    let crates = match r#type {
        DerivationType::Binaries { binaries, .. } => Either::Right(
            binaries
                .iter()
                .map(|bin| Crate::from_binary(bin, &node.dependency_renames)),
        ),
        DerivationType::Library { library, .. } => Either::Left(iter::once(
            Crate::from_library(library, &node.dependency_renames),
        )),
        DerivationType::BuildScript(build_script) => {
            Either::Left(iter::once(Crate::from_build_script(build_script)))
        },
    };

    let mut build_phase = "runHook preBuild\nmkdir -p $out\n".to_owned();

    let deps_path = deps.out_path(ctx)?.display().to_string();

    for cr8 in crates {
        build_phase.push_str("\nrustc");

        for rustc_arg in build_rustc_args(
            cr8,
            direct_deps.clone(),
            &node.package_attrs.features,
            is_release,
            target,
            node.package_attrs.edition,
            ctx,
        ) {
            build_phase.push(' ');
            build_phase.push_str(rustc_arg.as_ref());
        }

        build_phase.push_str("-L dependency=");
        build_phase.push_str(&deps_path);

        // Append any extra arguments coming from build scripts.
        build_phase.push_str(" ${EXTRA_RUSTC_ARGS:-}");
    }

    build_phase.push_str("\nrunHook postBuild");

    Ok(build_phase)
}

#[expect(clippy::too_many_arguments)]
fn configure_phase(
    package: &PackageAttrs,
    build_script: Option<NixDerivation>,
    is_library: bool,
    is_release: bool,
    stdenv: NixAttrset,
    ctx: &mut Context,
) -> Result<String> {
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
    let host_platform = stdenv.get::<NixAttrset>(c"hostPlatform", ctx)?;

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
    let target_pointer_width = if host_platform.get::<bool>(c"isILP32", ctx)? {
        32
    } else {
        cpu_platform.get::<u8>(c"bits", ctx)?
    };
    let target_vendor = host_platform
        .get::<NixAttrset>(c"parsed", ctx)?
        .get::<NixAttrset>(c"vendor", ctx)?
        .get::<CompactString>(c"name", ctx)?;

    let manifest_links = package.links.as_deref().unwrap_or("");

    let pkg_authors =
        package.authors.iter().fold(String::new(), |mut acc, author| {
            if !acc.is_empty() {
                acc.push(':');
            }
            acc.push_str(author);
            acc
        });
    let pkg_description = package
        .description
        .as_deref()
        .map_or(Cow::Borrowed(""), |desc| shell_escape::escape(desc.into()));
    let pkg_homepage = package.homepage.as_deref().unwrap_or("");
    let pkg_license = package.license.as_deref().unwrap_or("");
    let pkg_license_file = package.license_file.as_deref().unwrap_or("");
    let pkg_name = &*package.name;
    let pkg_readme = package.readme.as_deref().unwrap_or("");
    let pkg_repository = package.repository.as_deref().unwrap_or("");
    let pkg_rust_version = package.rust_version.as_deref().unwrap_or("");
    let pkg_version = &*package.version.to_compact_string();
    let pkg_version_major = package.version.major;
    let pkg_version_minor = package.version.minor;
    let pkg_version_patch = package.version.patch;
    let pkg_version_pre = package.version.pre.as_str();

    let debug = if is_release { "1" } else { "" };
    let host = stdenv
        .get::<NixAttrset>(c"buildPlatform", ctx)?
        .get::<NixAttrset>(c"rust", ctx)?
        .get::<CompactString>(c"rustcTargetSpec", ctx)?;
    let opt_level = if is_release { 3 } else { 0 };
    let profile = if is_release { "release" } else { "debug" };
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

    if let Some(build_script) = build_script {
        let build_script_out_path = build_script.out_path(ctx)?;

        writeln!(
            &mut configure_phase,
            "export OUT_DIR={}/out",
            build_script_out_path.display()
        )
        .expect("writing to string can't fail");

        writeln!(
            &mut configure_phase,
            "source {}/{}",
            build_script_out_path.display(),
            if is_library { "lib.sh" } else { "bin.sh" }
        )
        .expect("writing to string can't fail");
    }

    configure_phase.push_str("runHook postConfigure");

    Ok(configure_phase)
}

fn install_phase(package: &PackageAttrs, is_build_script: bool) -> String {
    let mut install_phase = "runHook preInstall\n".to_owned();

    if is_build_script {
        for feature in &package.features {
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
            package_name = package.name,
            package_version = package.version.to_compact_string(),
        ));
    }

    install_phase.push_str("runHook postInstall\n");

    install_phase
}

#[expect(clippy::too_many_arguments)]
fn build_rustc_args<'dep, Deps>(
    cr8: Crate,
    direct_deps: Deps,
    features: &[CompactString],
    is_release: bool,
    target: Option<&CompileTarget>,
    edition: Edition,
    ctx: &mut Context,
) -> impl Iterator<Item = impl AsRef<str>>
where
    Deps: Iterator<Item = (&'dep BuildGraphNode, NixDerivation<'dep>)>,
{
    [
        cr8.path,
        "--crate-name",
        cr8.name,
        "--out-dir",
        "$out",
        "--edition",
        edition_as_str(edition),
        "--cap-lints allow", // Suppress all lints from dependencies.
        "--remap-path-prefix $NIX_BUILD_TOP=/",
        "--color always",
        "--codegen",
        if is_release { "opt-level=3" } else { "debuginfo=2" },
        "--codegen",
    ]
    .into_iter()
    .map(Into::into)
    .chain([
        format_compact!(
            "codegen-units={}",
            cr8.build_opts.codegen_units.unwrap_or(1)
        ),
        CompactString::const_new("--crate-type"),
        cr8.types_arg,
    ])
    .chain(
        cr8.is_proc_macro
            .then(|| CompactString::const_new("--extern proc-macro")),
    )
    .chain(dependencies_rustc_args(direct_deps, cr8.deps_renames, ctx))
    .chain(
        (match target {
            Some(target) if cr8.is_compiled_for_host => Some([
                CompactString::const_new("--target"),
                target.rustc_target().as_str().into(),
            ]),
            _ => None,
        })
        .into_iter()
        .flatten(),
    )
    .chain(features.iter().flat_map(|feature| {
        [
            CompactString::const_new("--cfg"),
            format_compact!("feature=\\\"{}\\\"", feature),
        ]
    }))
    // TODO: set linker.
    .chain(cr8.build_opts.extra_rustc_args.iter().cloned())
}

fn dependencies_rustc_args<'dep, Deps>(
    dependencies: Deps,
    renames: &DependencyRenames,
    ctx: &mut Context,
) -> impl IntoIterator<Item = CompactString>
where
    Deps: Iterator<Item = (&'dep BuildGraphNode, NixDerivation<'dep>)>,
{
    dependencies
        .into_iter()
        .map(|(dep_node, dep_drv)| {
            let dep_lib = dep_node
                .library
                .as_ref()
                .expect("only library crates can be dependencies");

            let lib_name = match renames.get(&dep_node.package_attrs.name) {
                Some(DependencyRename::Simple(rename)) => rename,
                Some(DependencyRename::Extended(renames)) => renames
                    .iter()
                    .find_map(|RenameWithVersion { rename, version_req }| {
                        (version_req.matches(&dep_node.package_attrs.version))
                            .then(|| rename)
                    })
                    .unwrap_or_else(|| &dep_lib.name),
                None => &dep_lib.name,
            }
            .clone();

            let out_path = dep_drv
                .out_path(ctx)
                .expect("dependency derivation must have an output path");

            let lib_path = format!(
                "{}/lib{}.{}",
                out_path.display(),
                dep_lib.name,
                if dep_lib.is_proc_macro() { DLL_EXTENSION } else { "rlib" }
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

impl<'args, 'lock, 'builtins> GlobalArgs<'args, 'lock, 'builtins> {
    pub(crate) fn new(
        args: &BuildPackageArgs<'args>,
        vendored_sources: &'args VendoredSources<'lock>,
        ctx: &mut Context<'builtins>,
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

        let host_platform = stdenv.get::<NixAttrset>(c"hostPlatform", ctx)?;

        let host_config = host_platform.get::<CompactString>(c"config", ctx)?;

        let build_config = stdenv
            .get::<NixAttrset>(c"buildPlatform", ctx)?
            .get::<CompactString>(c"config", ctx)?;

        let compile_target = if host_config == build_config {
            None
        } else {
            let target = host_platform
                .get::<NixAttrset>(c"rust", ctx)?
                .get::<CompactString>(c"rustcTargetSpec", ctx)?;
            Some(
                CompileTarget::new(&target)
                    .expect("rustcTargetSpec is a valid target"),
            )
        };

        Ok(Self {
            compile_target,
            crate_overrides: args.crate_overrides,
            global_overrides: args.global_overrides,
            mk_derivation: stdenv.get::<NixLambda>(c"mkDerivation", ctx)?,
            mk_path: ctx.builtins().path(ctx),
            parse_build_script_output,
            release: args.release,
            rustc,
            stdenv,
            vendored_sources,
        })
    }
}

impl<'a> DerivationType<'a> {
    fn build_script_drv(&self) -> Option<NixDerivation<'a>> {
        match self {
            Self::BuildScript(_) => None,
            Self::Library { build_script, .. } => build_script.clone(),
            Self::Binaries { build_script, .. } => build_script.clone(),
        }
    }

    fn derivation_name_suffix(&self) -> &'static str {
        match self {
            Self::BuildScript(_) => "build",
            Self::Library { .. } => "lib",
            Self::Binaries { binaries, .. } if binaries.len() > 1 => "bins",
            Self::Binaries { .. } => "bin",
        }
    }

    fn is_build_script(&self) -> bool {
        matches!(self, Self::BuildScript(_))
    }

    fn is_library(&self) -> bool {
        matches!(self, Self::Library { .. })
    }

    fn library_drv(&self) -> Option<NixDerivation<'a>> {
        match self {
            Self::Binaries { library, .. } => library.clone(),
            _ => None,
        }
    }
}

impl<'a> Crate<'a> {
    fn from_binary(
        binary: &'a BinaryCrate,
        deps_renames: &'a DependencyRenames,
    ) -> Self {
        Self {
            path: &binary.path,
            name: &binary.name,
            types_arg: CompactString::const_new("bin"),
            is_proc_macro: false,
            is_compiled_for_host: true,
            deps_renames,
            build_opts: &binary.build_opts,
        }
    }

    fn from_build_script(build_script: &'a BuildScript) -> Self {
        Self {
            path: &build_script.path,
            name: "build_script_build",
            types_arg: CompactString::const_new("bin"),
            is_proc_macro: false,
            is_compiled_for_host: false,
            deps_renames: &build_script.dependency_renames,
            build_opts: &build_script.build_opts,
        }
    }

    fn from_library(
        library: &'a LibraryCrate,
        deps_renames: &'a DependencyRenames,
    ) -> Self {
        let types_arg = library.formats.iter().fold(
            CompactString::default(),
            |mut acc, format| {
                if !acc.is_empty() {
                    acc.push(',');
                }
                acc.push_str(format.as_str());
                acc
            },
        );

        let is_proc_macro = library.is_proc_macro();

        Self {
            path: &library.path,
            name: &library.name,
            types_arg,
            is_proc_macro,
            is_compiled_for_host: !is_proc_macro,
            deps_renames,
            build_opts: &library.build_opts,
        }
    }
}
