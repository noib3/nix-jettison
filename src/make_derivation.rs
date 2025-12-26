use core::fmt::Write;
use core::iter;
use std::borrow::Cow;
use std::env::consts::DLL_EXTENSION;

use cargo::core::Edition;
use cargo::core::compiler::CompileTarget;
use compact_str::{CompactString, ToCompactString, format_compact};
use either::Either;
use indoc::{formatdoc, writedoc};
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

    /// The `pkgs.lib.getLib` function.
    pub(crate) get_lib: NixLambda<'args>,

    /// The
    /// [`BuildPackageArgs::global_overrides`](crate::build_package::BuildPackageArgs::global_overrides) field.
    pub(crate) global_overrides: Option<NixAttrset<'args>>,

    /// The `pkgs.stdenv.mkDerivation` function.
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

#[expect(clippy::too_many_lines)]
pub(crate) fn make_deps<'dep>(
    package: &PackageAttrs,
    direct_deps: impl ExactSizeIterator<Item = NixDerivation<'dep>> + Clone,
    args: &GlobalArgs,
    ctx: &mut Context,
) -> Result<NixDerivation<'static>> {
    let mut install_phase = formatdoc!(
        "
            runHook preInstall
            mkdir -p $out
            mkdir -p $out/native
            shopt -s nullglob
        "
    )
    .to_owned();

    // For every direct dependency, copy all its native dependencies, pure-Rust
    // dependencies, and its output .rlib/.so/.dylib files (if any).
    for rust_dep in direct_deps.clone() {
        let out_path = rust_dep.out_path_as_string(ctx)?;
        writedoc!(
            &mut install_phase,
            r#"
                cp -rn {out_path}/deps/native/. $out/native
                cp -rn {out_path}/deps/. $out

                for dep in {out_path}/lib*.{{rlib,{DLL_EXTENSION}}}; do
                  ln -sf $dep $out
                done
            "#,
        )
        .expect("writing to string can't fail");
    }

    let build_inputs = apply_overrides(
        package,
        args.global_overrides,
        args.crate_overrides,
        ctx,
    )?
    .map(|attrs| attrs.get_opt::<NixList>(c"buildInputs", ctx))
    .transpose()?
    .flatten();

    // For every library in the buildInputs, symlink any .so/.dylib/.a files in
    // it under `native`.
    if let Some(list) = &build_inputs {
        for idx in 0..list.len() {
            let build_input = list.get::<NixDerivation>(idx, ctx)?;

            let native_dep = args
                .get_lib
                .call(build_input, ctx)?
                .force_into::<NixDerivation>(ctx)?;

            let out_path = native_dep.out_path_as_string(ctx)?;

            writedoc!(
                &mut install_phase,
                r#"
                    if [ -d "{out_path}/lib" ]; then
                      for dep in "{out_path}/lib"/lib*.{{so,so.*,dylib,a}}; do
                        ln -sf $dep $out/native
                      done
                    fi

                    # Only check lib64/ if it's not a symlink. If it is it'll point
                    # to lib/, so we can skip it.
                    if [ -d "{out_path}/lib64" ] && [ ! -L "{out_path}/lib64" ]; then
                      for dep in "{out_path}/lib64"/lib*.{{so,so.*,dylib,a}}; do
                        ln -sf $dep $out/native
                      done
                    fi
                "#,
            )
            .expect("writing to string can't fail");
        }
    }

    install_phase.push_str("runHook postInstall");

    let attrs = attrset! {
        name: format_compact!("{}-{}-deps", package.name, package.version),
        buildInputs: direct_deps.concat(build_inputs.into_list()).into_value(),
        installPhase: install_phase,
        phases: [ c"installPhase" ],
    };

    args.mk_derivation.call(attrs, ctx)?.force_into(ctx)
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
    Deps: ExactSizeIterator<Item = (&'a BuildGraphNode, NixDerivation<'a>)>
        + Clone,
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
    Deps: ExactSizeIterator<Item = (&'a BuildGraphNode, NixDerivation<'a>)>
        + Clone,
{
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

    let build_script_drv = r#type.build_script_drv();

    let configure_phase = configure_phase(
        &node.package_attrs,
        build_script_drv,
        r#type.is_library(),
        args.release,
        deps,
        args.stdenv,
        ctx,
    )?;

    let build_phase = build_phase(
        &r#type,
        node,
        direct_deps.clone(),
        args.release,
        args.compile_target.as_ref(),
        ctx,
    )?;

    let install_phase =
        install_phase(&node.package_attrs, r#type.is_build_script());

    let overrides = apply_overrides(
        &node.package_attrs,
        args.global_overrides,
        args.crate_overrides,
        ctx,
    )?;

    let extra_native_build_inputs = overrides
        .map(|attrs| attrs.get_opt::<NixList>(c"nativeBuildInputs", ctx))
        .transpose()?
        .flatten();

    let extra_build_inputs = overrides
        .map(|attrs| attrs.get_opt::<NixList>(c"buildInputs", ctx))
        .transpose()?
        .flatten();

    Ok(attrset! {
        name: derivation_name,
        src,
        configurePhase: configure_phase,
        buildPhase: build_phase,
        installPhase: install_phase,
        dontStrip: true,
        // See https://github.com/NixOS/nixpkgs/issues/218712.
        stripExclude: [ c"*.rlib" ],
        version: node.package_attrs.version.to_compact_string(),
    }
    .merge(overrides)
    .merge(attrset! {
        nativeBuildInputs: [args.parse_build_script_output, args.rustc]
            .concat(extra_native_build_inputs.into_list())
            .into_value(),
        buildInputs: build_script_drv
            .into_iter()
            .chain_exact(r#type.library_drv())
            .chain_exact(iter::once(deps.clone()))
            .chain_exact(direct_deps.clone().map(|(_node, drv)| drv))
            .concat(extra_build_inputs.into_list())
            .into_value(),
    }))
}

#[expect(clippy::too_many_arguments)]
fn build_phase<'dep, Deps>(
    r#type: &DerivationType,
    node: &BuildGraphNode,
    direct_deps: Deps,
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

    let mut build_phase = "runHook preBuild\n".to_owned();

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

        build_phase
            .push_str(" -L dependency=$out/deps -L native=$out/deps/native");

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
    deps: NixDerivation,
    stdenv: NixAttrset,
    ctx: &mut Context,
) -> Result<String> {
    let host_platform = stdenv.get::<NixAttrset>(c"hostPlatform", ctx)?;

    let cpu_platform =
        host_platform.get::<NixAttrset>([c"parsed", c"cpu"], ctx)?;

    let rust_platform =
        host_platform.get::<NixAttrset>([c"rust", c"platform"], ctx)?;

    let is_cpu_little_endian = cpu_platform
        .get::<CompactString>([c"significantByte", c"name"], ctx)?
        == "littleEndian";

    let target_arch = rust_platform.get::<CompactString>(c"arch", ctx)?;
    let target_endian = if is_cpu_little_endian { "little" } else { "big" };
    let target_os = rust_platform.get::<CompactString>(c"os", ctx)?;
    let target_pointer_width = if host_platform.get(c"isILP32", ctx)? {
        32
    } else {
        cpu_platform.get::<u8>(c"bits", ctx)?
    };
    let target_vendor = host_platform
        .get::<CompactString>([c"parsed", c"vendor", c"name"], ctx)?;

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
    let host = stdenv.get::<CompactString>(
        [c"buildPlatform", c"rust", c"rustcTargetSpec"],
        ctx,
    )?;
    let opt_level = if is_release { 3 } else { 0 };
    let profile = if is_release { "release" } else { "debug" };
    let target = host_platform
        .get::<CompactString>([c"rust", c"rustcTargetSpec"], ctx)?;

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

    writeln!(
        &mut configure_phase,
        "mkdir -p $out\nln -s {} $out/deps",
        deps.out_path_as_string(ctx)?,
    )
    .expect("writing to string can't fail");

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

    install_phase.push_str("runHook postInstall");

    install_phase
}

fn apply_overrides<'a>(
    package: &PackageAttrs,
    global_overrides: Option<NixAttrset<'a>>,
    crate_overrides: Option<NixAttrset<'a>>,
    ctx: &mut Context,
) -> Result<Option<NixAttrset<'a>>> {
    let Some(crate_overrides) = crate_overrides else {
        return Ok(global_overrides);
    };

    let Some(override_fun) =
        crate_overrides.get_opt::<NixLambda>(&package.name, ctx)?
    else {
        return Ok(global_overrides);
    };

    let attrs = global_overrides.merge(attrset! {
        version: package.version.to_compact_string(),
    });

    override_fun.call(attrs, ctx)?.force_into(ctx).map(Some)
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
            .then(|| CompactString::const_new("--extern proc_macro")),
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

        let build_config =
            stdenv.get::<CompactString>([c"buildPlatform", c"config"], ctx)?;

        let compile_target = if host_config == build_config {
            None
        } else {
            let target = host_platform
                .get::<CompactString>([c"rust", c"rustcTargetSpec"], ctx)?;
            Some(
                CompileTarget::new(&target)
                    .expect("rustcTargetSpec is a valid target"),
            )
        };

        Ok(Self {
            compile_target,
            crate_overrides: args.crate_overrides,
            get_lib: args.pkgs.get([c"lib", c"getLib"], ctx)?,
            global_overrides: args.global_overrides,
            mk_derivation: stdenv.get(c"mkDerivation", ctx)?,
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
