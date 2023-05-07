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
      pkgs = import nixpkgs {
        inherit system;
      };

      craneLib = crane.lib.${system};

      # Common derivation arguments used for all builds
      commonArgs = {
        src = craneLib.cleanCargoSource ../.;

        nativeBuildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [
          pkgs.libiconv
        ];
      };

      # Build *just* the cargo dependencies, so we can reuse
      # all of that work (e.g. via cachix) when running in CI
      cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      # Run clippy (and deny all warnings) on the crate source,
      # resuing the dependency artifacts (e.g. from build scripts or
      # proc-macros) from above.
      #
      # Note that this is done as a separate derivation so it
      # does not impact building just the crate by itself.
      myCrateClippy = craneLib.cargoClippy (commonArgs
        // {
          # Again we apply some extra arguments only to this derivation
          # and not every where else. In this case we add some clippy flags
          inherit cargoArtifacts;
          cargoClippyExtraArgs = "-- --deny warnings";
        });

      # Next, we want to run the tests and collect code-coverage, _but only if
      # the clippy checks pass_ so we do not waste any extra cycles.
      myCrateCoverage = craneLib.cargoNextest (commonArgs
        // {
          cargoArtifacts = myCrateClippy;
        });

      # Build the actual crate itself, reusing the dependency
      # artifacts from above.
      myCrate = craneLib.buildPackage (commonArgs
        // {
          inherit cargoArtifacts;
        });

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
        myCrate
        myCrateClippy
        myCrateCoverage
        pre-commit-check
        ;
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
