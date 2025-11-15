{ ... }:

{
  perSystem =
    { pkgs, ... }:
    {
      _module.args.common =
        let
          mkNixVersion = targetPkgs: targetPkgs.nixVersions.nix_2_32;
          mkBuildInputs = targetPkgs: [ (mkNixVersion targetPkgs).dev ];
          mkEnv = targetPkgs: {
            LIBCLANG_PATH = "${targetPkgs.llvmPackages.libclang.lib}/lib";
          };
        in
        {
          inherit mkBuildInputs mkEnv;
          buildInputs = mkBuildInputs pkgs;
          env = mkEnv pkgs;
          nativeBuildInputs = [ pkgs.pkg-config ];
          nixVersion = mkNixVersion pkgs;
        };
    };
}
