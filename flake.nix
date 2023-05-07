{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs";
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    crane,
  }: let
    forEachSystem = nixpkgs.lib.genAttrs [
      "aarch64-darwin"
      "aarch64-linux"
      "x86_64-darwin"
      "x86_64-linux"
    ];
  in {
    packages = forEachSystem (system: let
      overlay = next: prev: {
        inherit
          (prev.callPackage ./default.nix {
            pkgs = prev;
            craneLib = crane.lib.${system};
          })
          myCrate
          ;
      };

      pkgs = import nixpkgs {
        inherit system;
        overlays = [overlay];
      };
    in {
      default = pkgs.myCrate;
    });
  };
}
