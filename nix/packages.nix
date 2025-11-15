{ inputs, ... }:

{
  imports = [
    ./common.nix
    ./rust.nix
  ];

  perSystem =
    {
      pkgs,
      lib,
      common,
      rust,
      ...
    }:
    let
      packageName = (lib.importTOML ../crates/nix-jettison/Cargo.toml).package.name;

      # Generate the Cargo.nix using IFD.
      mkCargoNix =
        targetPkgs:
        inputs.crate2nix.tools.${targetPkgs.stdenv.hostPlatform.system}.appliedCargoNix {
          name = packageName;
          src = ../.;
        };

      mkBuild =
        {
          release ? true,
          targetPkgs ? pkgs,
        }:
        (mkCargoNix targetPkgs).override {
          pkgs = targetPkgs;
          inherit release;
          buildRustCrateForPkgs =
            crate:
            targetPkgs.buildRustCrate.override {
              rustc = rust.mkToolchain targetPkgs;
              cargo = rust.mkToolchain targetPkgs;
            };
          defaultCrateOverrides = targetPkgs.defaultCrateOverrides // {
            nix-bindings-sys = attrs: {
              inherit (common) nativeBuildInputs;
              buildInputs = common.mkBuildInputs targetPkgs;
              env = common.mkEnv targetPkgs;
            };
          };
        };

      mkPackage =
        {
          release ? true,
          targetPkgs ? pkgs,
        }@args:
        let
          build = mkBuild args;
          nixJettison = build.workspaceMembers.${packageName}.build.lib;
          dylibName = builtins.replaceStrings [ "-" ] [ "_" ] packageName;
          dylibExt = if targetPkgs.stdenv.isDarwin then "dylib" else "so";
        in
        pkgs.runCommand "${packageName}${lib.optionalString (!release) "-dev"}" { } ''
          mkdir -p $out
          src=$(readlink -f ${nixJettison}/lib/lib${dylibName}.${dylibExt})
          cp $src $out/${dylibName}.so
        '';
    in
    {
      packages = {
        default = mkPackage { release = true; };
        dev = mkPackage { release = false; };
      };
    };
}
