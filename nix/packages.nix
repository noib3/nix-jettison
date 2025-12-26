{ self, ... }:

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
      dllName = builtins.replaceStrings [ "-" ] [ "_" ] packageName;
      dllSuffix = if pkgs.stdenv.isDarwin then "dylib" else "so";

      mkPackage =
        {
          release ? true,
          targetPkgs ? pkgs,
        }:
        let
          inherit (common) nativeBuildInputs;
          buildInputs = common.mkBuildInputs targetPkgs;
          env = common.mkEnv targetPkgs;
          jettison = self.lib.buildPackage {
            pkgs = targetPkgs;
            src = ../.;
            inherit release;
            rustc = rust.mkToolchain targetPkgs;
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
          };
        in
        pkgs.runCommand "${packageName}${lib.optionalString (!release) "-dev"}" { } ''
          mkdir -p $out
          src=$(readlink -f ${jettison}/lib${dllName}.${dllSuffix})
          cp $src $out/${dllName}.so
        '';

      bootstrapped =
        let
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rust.mkToolchain pkgs;
            rustc = rust.mkToolchain pkgs;
          };
          cargoToml = lib.importTOML ../Cargo.toml;
          jettison = rustPlatform.buildRustPackage {
            pname = packageName;
            version = cargoToml.workspace.package.version;
            src = lib.fileset.toSource {
              root = ../.;
              fileset = lib.fileset.fileFilter (
                file:
                (lib.hasSuffix ".rs" file.name)
                || (lib.hasSuffix ".cpp" file.name)
                || (file.name == "Cargo.toml")
                || (file.name == "Cargo.lock")
              ) ../.;
            };
            cargoLock = {
              lockFile = ../Cargo.lock;
              outputHashes = {
                "cauchy-0.1.0" = "sha256-3Z4yHxAnscoysPYPfx9ULMLDS6uaJUkte9IPcnwrbOE=";
              };
            };
            inherit (common) nativeBuildInputs;
            buildInputs = common.mkBuildInputs pkgs ++ [ pkgs.curl.dev ];
            env = common.mkEnv pkgs;
            buildType = "release";
          };
        in
        pkgs.runCommand "${packageName}-bootstrapped" { } ''
          mkdir -p $out
          src=$(readlink -f ${jettison}/lib/lib${dllName}.${dllSuffix})
          cp $src $out/${dllName}.so
        '';
    in
    {
      packages = {
        inherit bootstrapped;
        default = mkPackage { release = true; };
        dev = mkPackage { release = false; };
      };
    };
}
