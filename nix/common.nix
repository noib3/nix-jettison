{ ... }:

{
  perSystem =
    { pkgs, ... }:
    {
      _module.args.common =
        let
          mkNixVersion = targetPkgs: targetPkgs.nixVersions.nix_2_32;
          mkBuildInputs = targetPkgs: [ (mkNixVersion targetPkgs).dev ];
        in
        {
          inherit mkBuildInputs;
          buildInputs = mkBuildInputs pkgs;
          nativeBuildInputs = [ pkgs.pkg-config ];
          nixVersion = mkNixVersion pkgs;
        };
    };
}
