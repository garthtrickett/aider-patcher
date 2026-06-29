{
  description = "Aider Patcher binary";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        # 1. Derivation for the Rust patcher binary
        aider-patcher = pkgs.rustPlatform.buildRustPackage {
          pname = "aider-patcher";
          version = "0.1.2";
          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ ];
        };

      in
      {
        packages = {
          inherit aider-patcher;
          default = aider-patcher;
        };

        # Developer shell for local testing & development
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            cargo
            rustc
            rustfmt
            clippy
            git
          ];
        };
      }
    );
}
