{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    crate2nix = {
      url = "github:nix-community/crate2nix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.flake-parts.follows = "flake-parts";
    };

    flake-parts.url = "github:hercules-ci/flake-parts";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
        "x86_64-linux"
      ];

      imports = [
        ./nix/lib.nix
        ./nix/packages.nix
        ./nix/repl.nix
        ./nix/shell.nix
      ];

      flake = {
        darwinModules.default = import ./nix/module.nix inputs.self;
        homeManagerModules.default = import ./nix/module.nix inputs.self;
        nixosModules.default = import ./nix/module.nix inputs.self;
      };
    };
}
