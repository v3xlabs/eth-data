{
  description = "eth-data: a read-only MCP server for Ethereum data";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [rust-overlay.overlays.default];
      };

      rustToolchain = pkgs.rust-bin.stable.latest.default.override {
        extensions = [
          "rust-src"
          "llvm-tools"
        ];
      };

      rustfmtNightly = pkgs.rust-bin.nightly.latest.rustfmt;

      rustPlatform = pkgs.makeRustPlatform {
        cargo = rustToolchain;
        rustc = rustToolchain;
      };

      unwrapped = pkgs.callPackage ./nix/package.nix {
        inherit rustPlatform;
        withAnvil = false;
      };

      wrapped = pkgs.callPackage ./nix/package.nix {
        inherit rustPlatform;
        withAnvil = true;
      };
    in {
      packages =
        {
          eth-data-mcp = wrapped;
          eth-data-mcp-unwrapped = unwrapped;
          default = wrapped;
        }
        // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
          eth-data-mcp-sandboxed = pkgs.callPackage ./nix/sandbox.nix {
            eth-data-mcp = unwrapped;
          };
        };

      checks = {
        inherit (self.packages.${system}) eth-data-mcp eth-data-mcp-unwrapped;
      };

      apps.default = {
        type = "app";
        program = "${wrapped}/bin/eth-data-mcp";
      };

      devShells.default = pkgs.mkShell {
        packages = with pkgs; [
          rustfmtNightly
          rustToolchain
          rust-analyzer
          bacon
          just

          foundry
          pkg-config
        ];
      };

      formatter = pkgs.alejandra;
    })
    // {
      overlays.default = final: prev: {
        eth-data-mcp = final.callPackage ./nix/package.nix {};
      };
    };
}
