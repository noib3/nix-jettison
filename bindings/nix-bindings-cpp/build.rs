#![allow(missing_docs)]

fn main() {
    let nix_expr = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("nix-expr")
        .expect("Could not find nix-expr via pkg-config");

    let mut build = cc::Build::new();

    for lib in [&nix_expr] {
        for include_path in &lib.include_paths {
            build.include(include_path);
        }
    }

    build
        .cpp(true)
        .file("cpp/wrapper.cpp")
        .flag("-std=c++23")
        .compile("nix_bindings_cpp");

    println!("cargo:rerun-if-changed=cpp/wrapper.cpp");
}
