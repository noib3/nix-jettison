#![allow(missing_docs)]

// Note: When building in Nix on macOS, you may see a warning:
// "Warning: supplying the --target arm64-apple-macosx != arm64-apple-darwin..."
// This is harmless - the cc crate converts the target to Apple's SDK
// format (arm64-apple-macosx) while Nix expects the Rust target format
// (aarch64-apple-darwin).
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
