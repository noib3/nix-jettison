# nix-jettison

`nix-jettison` lets you build Rust projects in Nix, producing one derivation
per-crate in the dependency graph, and it does so without IFD or the need to
maintain a pre-generated `Cargo.nix` file.

It works by compiling down to a shared library that can be dynamically loaded
by Nix via the [`plugin-files`][plugin-files] option. Once loaded, it adds a
new `builtins.jettison` table that exposes the library's API.

The goal is to combine all the best features of similar projects like [Naersk],
[Crane], [cargo2nix], [crate2nix], etc; without having to port any of Cargo's
logic to Nix.

Status: ongoing development, not yet ready for production use.

[Crane]: https://github.com/ipetkov/crane/
[Naersk]: https://github.com/nix-community/naersk
[cargo2nix]: https://github.com/cargo2nix/cargo2nix/
[crate2nix]: https://github.com/nix-community/crate2nix/
[plugin-files]: https://nix.dev/manual/nix/2.32/command-ref/conf-file#conf-plugin-files
