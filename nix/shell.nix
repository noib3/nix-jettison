{ ... }:

{
  imports = [
    ./common.nix
    ./rust.nix
  ];

  perSystem =
    {
      pkgs,
      common,
      rust,
      ...
    }:
    {
      devShells.default = pkgs.mkShell {
        inherit (common) buildInputs;

        packages = common.nativeBuildInputs ++ [
          (pkgs.rustfmt.override { asNightly = true; })
          (rust.toolchain.override {
            extensions = [
              "clippy"
              "rust-analyzer"
            ];
          })
        ];
      };
    };
}
