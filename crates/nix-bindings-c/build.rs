use std::env;
use std::path::PathBuf;

use bindgen::callbacks::ParseCallbacks;

#[derive(Debug)]
struct ProcessComments;

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

fn main() {
    println!("cargo:rustc-link-lib=nixstorec");
    println!("cargo:rustc-link-lib=nixutilc");
    println!("cargo:rustc-link-lib=nixexprc");
    println!("cargo:rustc-link-lib=nixflakec");

    let store_c = pkg_config::Config::new()
        .atleast_version("2.0.0")
        .probe("nix-store-c")
        .expect("Could not find nix-store-c via pkg-config");

    let util_c = pkg_config::Config::new()
        .atleast_version("2.0.0")
        .probe("nix-util-c")
        .expect("Could not find nix-util-c via pkg-config");

    let expr_c = pkg_config::Config::new()
        .atleast_version("2.0.0")
        .probe("nix-expr-c")
        .expect("Could not find nix-expr-c via pkg-config");

    let flake_c = pkg_config::Config::new()
        .atleast_version("2.0.0")
        .probe("nix-flake-c")
        .expect("Could not find nix-flake-c via pkg-config");

    let mut builder = bindgen::Builder::default()
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .formatter(bindgen::Formatter::Rustfmt)
        .rustfmt_configuration_file(
            std::fs::canonicalize("../rustfmt.toml").ok(),
        )
        .parse_callbacks(Box::new(ProcessComments));

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

    for lib in [&store_c, &util_c, &expr_c, &flake_c] {
        for include_path in &lib.include_paths {
            builder =
                builder.clang_arg(format!("-I{}", include_path.display()));
        }
    }

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    let bindings = builder.generate().expect("Unable to generate bindings");
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
