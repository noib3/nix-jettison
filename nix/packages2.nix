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
      packageName = (lib.importTOML ../Cargo.toml).package.name;

      mkBuild =
        {
          release ? true,
          targetPkgs ? pkgs,
        }:
        builtins.jettison.buildPackage {
          package = packageName;
          pkgs = targetPkgs;
          src = ../.;
          crateOverrides = targetPkgs.defaultCrateOverrides // {
            nix-bindings-cpp = attrs: {
              inherit (common) nativeBuildInputs;
              buildInputs = common.mkBuildInputs targetPkgs;
              env = common.mkEnv targetPkgs;
            };
            nix-bindings-sys = attrs: {
              inherit (common) nativeBuildInputs;
              buildInputs = common.mkBuildInputs targetPkgs;
              env = common.mkEnv targetPkgs;
            };
            nix-jettison = attrs: {
              inherit (common) nativeBuildInputs;
              buildInputs = (common.mkBuildInputs targetPkgs) ++ [ targetPkgs.curl.dev ];
            };
          };
          inherit release;
          rustc = rust.mkToolchain targetPkgs;
        };

      mkPackage =
        {
          release ? true,
          targetPkgs ? pkgs,
        }@args:
        let
          build = mkBuild args;
          # nixJettison = build.workspaceMembers.${packageName}.build.lib;
          # dylibName = builtins.replaceStrings [ "-" ] [ "_" ] packageName;
          # dylibExt = if targetPkgs.stdenv.isDarwin then "dylib" else "so";
        in
        build;
      # pkgs.runCommand "${packageName}${lib.optionalString (!release) "-dev"}" { } ''
      #   mkdir -p $out
      #   src=$(readlink -f ${nixJettison}/lib/lib${dylibName}.${dylibExt})
      #   cp $src $out/${dylibName}.so
      # '';
    in
    {
      packages = {
        jettison-default = mkPackage { release = true; };
        jettison-dev = mkPackage { release = false; };
      };
    };
}
