#![allow(missing_docs)]

use std::env;
use std::path::PathBuf;

use bindgen::callbacks::{ItemInfo, ParseCallbacks};

fn main() {
    println!("cargo:rustc-link-lib=nixexprc");
    println!("cargo:rustc-link-lib=nixflakec");
    println!("cargo:rustc-link-lib=nixstorec");
    println!("cargo:rustc-link-lib=nixutilc");

    let expr_c = pkg_config::Config::new()
        .atleast_version("2.0.0")
        .probe("nix-expr-c")
        .expect("Could not find nix-expr-c via pkg-config");

    let flake_c = pkg_config::Config::new()
        .atleast_version("2.0.0")
        .probe("nix-flake-c")
        .expect("Could not find nix-flake-c via pkg-config");

    let store_c = pkg_config::Config::new()
        .atleast_version("2.0.0")
        .probe("nix-store-c")
        .expect("Could not find nix-store-c via pkg-config");

    let util_c = pkg_config::Config::new()
        .atleast_version("2.0.0")
        .probe("nix-util-c")
        .expect("Could not find nix-util-c via pkg-config");

    let mut builder = bindgen::Builder::default()
        .use_core()
        .formatter(bindgen::Formatter::Rustfmt)
        .rustfmt_configuration_file(
            std::fs::canonicalize("../rustfmt.toml").ok(),
        )
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .parse_callbacks(Box::new(ProcessComments))
        .parse_callbacks(Box::new(StripNixPrefix));

    builder = builder.header_contents(
        "wrapper.h",
        r#"
      #include <nix_api_expr.h>
      #include <nix_api_external.h>
      #include <nix_api_flake.h>
      #include <nix_api_store.h>
      #include <nix_api_util.h>
      #include <nix_api_value.h>
    "#,
    );

    for lib in [&expr_c, &flake_c, &store_c, &util_c] {
        for include_path in &lib.include_paths {
            builder =
                builder.clang_arg(format!("-I{}", include_path.display()));
        }
    }

    let out_path = PathBuf::from(
        env::var("OUT_DIR").expect("OUT_DIR is set in build scripts"),
    );

    let bindings = builder.generate().expect("Unable to generate bindings");
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}

#[derive(Debug)]
struct ProcessComments;

#[derive(Debug)]
struct StripNixPrefix;

impl ParseCallbacks for ProcessComments {
    fn process_comment(&self, comment: &str) -> Option<String> {
        match doxygen_bindgen::transform(comment) {
            Ok(res) => Some(res),
            Err(err) => {
                println!(
                    "cargo:warning=Problem processing doxygen comment: \
                     {comment}\n{err}"
                );
                None
            },
        }
    }
}

impl ParseCallbacks for StripNixPrefix {
    fn item_name(&self, item_info: ItemInfo<'_>) -> Option<String> {
        item_info.name.strip_prefix("nix_").map(ToOwned::to_owned)
    }
}
