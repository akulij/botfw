{
  # ...

  inputs = {
    # nixpkgs.url = "nixpkgs/nixos-unstable";
    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
    crate2nix.url = "github:nix-community/crate2nix";
    rust-overlay.url = "github:oxalica/rust-overlay";
    # ...
  };

  outputs =
    inputs @ { self
    , nixpkgs
    , flake-parts
    , rust-overlay
    , crate2nix
    , ...
    }: flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-darwin"
      ];

      perSystem = { system, pkgs, lib, inputs', ... }:
        let
          cargoNix = inputs.crate2nix.tools.${system}.appliedCargoNix {
            name = "gongbotrs";
            src = ./.;
          };
        in
        rec {
          checks = {
            rustnix = cargoNix.rootCrate.build.override {
              runTests = true;
            };
          };

          packages = {
            rustnix = cargoNix.rootCrate.build;
            default = packages.rustnix;
          };
        };
    };
}
