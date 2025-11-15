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
    nix.settings.plugin-files = [
      self.packages.${pkgs.stdenv.hostPlatform.system}.default
    ];
  };
}
