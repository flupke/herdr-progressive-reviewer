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
          # The Herdr release that the tests run, whatever Herdr is installed.
          # docs/development.md says how to change it.
          herdrVersion = "0.9.3";
          herdrAssets = {
            aarch64-darwin = {
              name = "herdr-macos-aarch64";
              hash = "sha256-UXOj4K5C1dGrfr+l1eYyn3w9I/jho2d8fOMjHaKIQVc=";
            };
            aarch64-linux = {
              name = "herdr-linux-aarch64";
              hash = "sha256-TeeqPiVniBLpKWDeZPfCqqG8ofD4CjxeVZg34jHh9cA=";
            };
            x86_64-darwin = {
              name = "herdr-macos-x86_64";
              hash = "sha256-22LVSP8+gysIepaxiUoI0mvjkF8YMDCc1VZ4PyFdQFQ=";
            };
            x86_64-linux = {
              name = "herdr-linux-x86_64";
              hash = "sha256-GKjcZfHC+khYhDRDVt6hz9kRxvBs9G+njhk/QIf026c=";
            };
          };
          testHerdr = pkgs.stdenvNoCC.mkDerivation {
            pname = "herdr";
            version = herdrVersion;
            src = pkgs.fetchurl {
              url = "https://github.com/herdrdev/herdr/releases/download/v${herdrVersion}/${herdrAssets.${system}.name}";
              inherit (herdrAssets.${system}) hash;
            };
            dontUnpack = true;
            # Keep the release binary byte for byte.
            dontFixup = true;
            installPhase = "install -Dm755 $src $out/bin/herdr";
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
              pkgs.rust-bin.stable."1.90.0".default
              pkgs.cargo-nextest
              pkgs.cargo-mutants
              pkgs.jq
              rustComplexityAnalyzer
              # The e2e tests of the Explore page (tests/explore-page) need Node 22.12 or later.
              pkgs.nodejs_22
            ];
            # Not on PATH: `herdr` there stays the installed Herdr, which
            # `make install` and the live server use.
            TEST_HERDR_BIN_PATH = "${testHerdr}/bin/herdr";
            # e2e sends usage data unless this is set.
            E2E_TELEMETRY_DISABLED = "1";
            # The browser the Explore page e2e tests attach to over CDP, instead of the build
            # Playwright downloads, which does not run on NixOS. Playwright's headless shell,
            # patched by nixpkgs: unlike the full Chromium, it does not call Google services.
            E2E_CHROMIUM = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux "${pkgs.playwright-driver.components.chromium-headless-shell}/chrome-headless-shell-linux64/chrome-headless-shell";
          };
        }
      );
    };
}
