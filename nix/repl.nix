{ self, ... }:

{
  perSystem =
    {
      pkgs,
      lib,
      common,
      ...
    }:
    {
      apps.repl = {
        type = "app";
        program = toString (
          pkgs.writeShellScript "repl" ''
            exec ${lib.getExe common.nixVersion} repl \
              --plugin-files ${self.packages.${pkgs.stdenv.hostPlatform.system}.default} \
              "$@"
          ''
        );
      };
    };
}
