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
        packages = self.packages.${pkgs.stdenv.hostPlatform.system};
      in
      [
        (if builtins ? jettison then packages.default else packages.bootstrapped)
      ];
  };
}
