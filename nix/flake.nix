{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs";
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    pre-commit-hooks = {
      url = "github:cachix/pre-commit-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    crane,
    pre-commit-hooks,
  }: let
    forEachSystem = nixpkgs.lib.genAttrs [
      "aarch64-darwin"
      "aarch64-linux"
      "x86_64-darwin"
      "x86_64-linux"
    ];
  in {
    checks = forEachSystem (system: let
      overlay = next: prev: {
        inherit
          (prev.callPackage ../default.nix {
            pkgs = prev;
            craneLib = crane.lib.${system};
          })
          myCrate
          myCrateClippy
          myCrateCoverage
          ;
      };

      pkgs = import nixpkgs {
        inherit system;
        overlays = [overlay];
      };

      pre-commit-check = pre-commit-hooks.lib.${system}.run {
        src = ../.;
        hooks = {
          actionlint.enable = true;
          alejandra.enable = true;
          prettier.enable = true;
          rustfmt.enable = true;
        };
      };
    in {
      inherit
        (pkgs)
        myCrate
        myCrateClippy
        myCrateCoverage
        ;
      inherit pre-commit-check;
    });

    devShells = forEachSystem (system: {
      default = nixpkgs.legacyPackages.${system}.mkShell {
        nativeBuildInputs = with nixpkgs.legacyPackages.${system}; [
          cargo
          clippy
          rust-analyzer
          rustc
          rustfmt
        ];

        inherit (self.checks.${system}.pre-commit-check) shellHook;
      };
    });
  };
}
