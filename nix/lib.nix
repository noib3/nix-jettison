{ ... }:

{
  flake = {
    lib =
      if builtins ? nix-jettison then
        builtins.nix-jettison
      else
        throw ''
          The nix-jettison plugin has not been loaded.

          To use this library, you need to configure Nix to load the nix-jettison plugin
          *before* your flake is evaluated.

          We provide NixOS, nix-darwin, and home-manager modules to configure Nix for you.
          For installation instructions and usage, see our documentation at:

            https://github.com/noib3/nix-jettison#installation
        '';
  };
}
