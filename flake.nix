{
  inputs = {
    cccc.url = "github:moznion/cccc/v1.6.0";
    cccc.flake = false;
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    { cccc, nixpkgs, rust-overlay, ... }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
        "x86_64-linux"
      ];
    in
    {
      devShells = nixpkgs.lib.genAttrs systems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          latestRustPlatform = pkgs.makeRustPlatform {
            cargo = pkgs.rust-bin.stable.latest.default;
            rustc = pkgs.rust-bin.stable.latest.default;
          };
          rustComplexityAnalyzer = latestRustPlatform.buildRustPackage {
            pname = "cccc-cli";
            version = "1.6.0";
            src = cccc;
            cargoLock = {
              lockFile = "${cccc}/Cargo.lock";
              outputHashes."tree-sitter-kotlin-0.4.0" =
                "sha256-O9zCo8G8321cpqVp4z8USwyoLtnJReNhWs1FpYa9IVQ=";
            };
            cargoBuildFlags = [ "--package=cccc-cli" ];
            cargoTestFlags = [ "--package=cccc-cli" ];
            buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.glibc.dev ];
            "BINDGEN_EXTRA_CLANG_ARGS_${pkgs.stdenv.hostPlatform.rust.rustcTarget}" =
              pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux (
                "-isystem ${pkgs.glibc.dev}/include"
              );
            nativeBuildInputs = [
              pkgs.git
              pkgs.rustPlatform.bindgenHook
            ];
          };
        in
        {
          default = pkgs.mkShell {
            packages = [
              pkgs.rust-bin.stable."1.88.0".default
              pkgs.cargo-nextest
              pkgs.cargo-mutants
              pkgs.jq
              rustComplexityAnalyzer
            ];
          };
        }
      );
    };
}
