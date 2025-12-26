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
        inherit (common) env;

        buildInputs = common.buildInputs ++ [
          pkgs.curl.dev
        ];

        packages = common.nativeBuildInputs ++ [
          (pkgs.rustfmt.override { asNightly = true; })
          (rust.toolchain.override {
            extensions = [
              "rust-analyzer"
              # Needed by rust-analyzer to index 'std'.
              "rust-src"
            ];
          })
        ];
      };
    };
}
