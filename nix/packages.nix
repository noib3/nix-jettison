{ ... }:

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

      mkPackage =
        {
          release ? true,
          targetPkgs ? pkgs,
        }:
        let
          inherit (common) nativeBuildInputs;
          buildInputs = common.mkBuildInputs targetPkgs;
          env = common.mkEnv targetPkgs;
          jettison = builtins.jettison.buildPackage {
            package = packageName;
            pkgs = targetPkgs;
            src = ../.;
            crateOverrides = targetPkgs.defaultCrateOverrides // {
              nix-bindings-cpp = attrs: {
                inherit nativeBuildInputs buildInputs env;
              };
              nix-bindings-sys = attrs: {
                inherit nativeBuildInputs buildInputs env;
              };
              nix-jettison = attrs: {
                inherit nativeBuildInputs;
                buildInputs = buildInputs ++ [ targetPkgs.curl.dev ];
              };
            };
            inherit release;
            rustc = rust.mkToolchain targetPkgs;
          };
          dllName = builtins.replaceStrings [ "-" ] [ "_" ] packageName;
          dllSuffix = if targetPkgs.stdenv.isDarwin then "dylib" else "so";
        in
        pkgs.runCommand "${packageName}${lib.optionalString (!release) "-dev"}" { } ''
          mkdir -p $out
          src=$(readlink -f ${jettison.lib}/lib/lib${dllName}.${dllSuffix})
          cp $src $out/${dllName}.so
        '';
    in
    {
      packages = {
        default = mkPackage { release = true; };
        dev = mkPackage { release = false; };
      };
    };
}
