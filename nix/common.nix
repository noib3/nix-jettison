{ ... }:

{
  perSystem =
    { pkgs, ... }:
    {
      _module.args.common =
        let
          mkBuildInputs = targetPkgs: [
            targetPkgs.nixVersions.nix_2_32.dev
          ];
        in
        {
          inherit mkBuildInputs;

          buildInputs = mkBuildInputs pkgs;

          nativeBuildInputs = [
            pkgs.pkg-config
          ];
        };
    };
}
