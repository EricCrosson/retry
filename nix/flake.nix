{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs";
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    fenix = {
      url = "github:nix-community/fenix";
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
    fenix,
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
      pkgs = import nixpkgs {inherit system;};
      craneDerivations = pkgs.callPackage ./default.nix {inherit crane fenix;};
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
        (craneDerivations)
        myCrate
        myCrateClippy
        myCrateCoverage
        ;
      inherit pre-commit-check;
    });

    devShells = forEachSystem (system: let
      pkgs = import nixpkgs {inherit system;};
      craneDerivations = pkgs.callPackage ./default.nix {inherit crane fenix;};
    in {
      default = pkgs.mkShell {
        packages = with pkgs;
          [
            craneDerivations.fenix-toolchain
          ]
          ++ craneDerivations.commonArgs.nativeBuildInputs;

        RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";

        inherit (self.checks.${system}.pre-commit-check) shellHook;
      };
    });
  };
}
