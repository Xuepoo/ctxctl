{
  description = "CtxCtl - pure CLI, zero-MCP, stateless context layer for AI coding agents";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rustToolchain = pkgs.rust-bin.stable."1.97.1".default.override {
          extensions = [ "rustfmt" "clippy" ];
        };
      in {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustToolchain

            # Tooling
            just
            lefthook
            cargo-nextest
            cargo-deny
            cargo-audit
            cargo-machete
            cargo-llvm-cov
            nodejs_22
            markdownlint-cli2
            actionlint
            act
          ];

          shellHook = ''
            echo "CtxCtl development shell"
            echo "Rust: $(rustc --version)"
          '';
        };
      });
}
