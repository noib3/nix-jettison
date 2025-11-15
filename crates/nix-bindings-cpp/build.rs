fn main() {
    // Get Nix include paths from pkg-config
    let nix_expr = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("nix-expr")
        .expect("Could not find nix-expr via pkg-config");

    let nix_store = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("nix-store")
        .expect("Could not find nix-store via pkg-config");

    let nix_util = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("nix-util")
        .expect("Could not find nix-util via pkg-config");

    let bdw_gc = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("bdw-gc")
        .expect("Could not find bdw-gc via pkg-config");

    // Compile C++ wrapper
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .file("cpp/wrapper.cpp")
        .flag("-std=c++23");

    // Note: When building in Nix on macOS, you may see a warning:
    // "Warning: supplying the --target arm64-apple-macosx != arm64-apple-darwin..."
    // This is harmless - the cc crate converts the target to Apple's SDK format
    // (arm64-apple-macosx) while Nix expects the Rust target format
    // (aarch64-apple-darwin). The build succeeds and produces correct binaries.

    // Add include paths
    for lib in [&nix_expr, &nix_store, &nix_util, &bdw_gc] {
        for include_path in &lib.include_paths {
            build.include(include_path);
        }
    }

    build.compile("nix_bindings_cpp");

    println!("cargo:rerun-if-changed=cpp/wrapper.cpp");
}
