self:

{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.nix.plugins.jettison;
in
{
  options.nix.plugins.jettison = {
    enable = lib.mkEnableOption "nix-jettison plugin";
  };

  config = lib.mkIf cfg.enable {
    nix.settings.plugin-files =
      let
        inherit (pkgs.stdenv) hostPlatform;
        packs = self.packages.${hostPlatform.system};
        package = if builtins ? jettison then packs.default else packs.bootstrapped;
        packageName = (lib.importTOML ../Cargo.toml).package.name;
        libName = builtins.replaceStrings [ "-" ] [ "_" ] packageName;
      in
      [
        "${package}/lib${libName}${hostPlatform.extensions.sharedLibrary}"
      ];
  };
}
