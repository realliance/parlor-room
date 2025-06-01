{
  description = "parlor-room";

  inputs = {
    nixpkgs.url = "nixpkgs";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url  = "github:numtide/flake-utils";

    crane = {
      url = "github:ipetkov/crane";
    };
  };

  # Based on https://github.com/oxalica/rust-overlay
  outputs = { self, nixpkgs, rust-overlay, flake-utils, crane, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        # Input pkgs
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        # Setup crane with toolchain
        rustToolchain = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # crane define src
        src = craneLib.cleanCargoSource ./.;

        nativeBuildInputs = [
          pkgs.pkg-config
        ];

        # build artifacts
        commonArgs = {
          inherit src nativeBuildInputs;
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        crate = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
        });
    in
    with pkgs;
    {
      devShells.default = mkShell {
        buildInputs = [
          rustToolchain
        ];
        nativeBuildInputs = [
          pkg-config
        ];
      };
      packages = {
        inherit crate;
        default = crate;
      };
    }
  );
}
