{
  description = "Deterministic Rust dev shell";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    devtools.url = "github:mineichen/nix-devtools";
  };
  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      perSystem = { system, ... }:
        let
          pkgs = import inputs.nixpkgs {
            inherit system;
            config.allowUnfreePredicate = pkg: builtins.elem (inputs.nixpkgs.lib.getName pkg) [ "claude-code" ];
          };
          rust = with inputs.fenix.packages.${system}; combine [
            stable.toolchain
          ];
          # cargo-fuzz needs a nightly toolchain (`-Z` sanitizer/coverage flags),
          # so the fuzzing shell uses a dedicated nightly toolchain.
          rustNightly = with inputs.fenix.packages.${system}; combine [
            latest.toolchain
          ];
          commonBuildInputs = [
            rust
	        pkgs.git
	        pkgs.stdenv.cc
          ];
          greet = ''
            echo "===================================="
            echo " Welcome to the deterministic dev shell! "
            echo "===================================="
            cargo --version
          '';
          isolated = inputs.devtools.lib.isolated {
            inherit pkgs inputs;
            name = "imask-isolated-dev";
            packages = commonBuildInputs ++ [
              pkgs.bashInteractive
              pkgs.ripgrep
              pkgs.opencode
              pkgs.busybox
            ];
            claude.pluginDirs = [
              (inputs.devtools.lib.claudeRustAnalyzer {
                inherit pkgs;
                rust-analyzer = rust;
              })
            ];
            shellHook = greet;
          };
        in
        {
          devShells.default = pkgs.mkShell({
            buildInputs = commonBuildInputs;
            shellHook = greet;
          });
          # Fuzzing shell: nightly toolchain + cargo-fuzz, with the shared
          # libraries cargo-fuzz needs at runtime (libz, libstdc++).
          devShells.fuzz = pkgs.mkShell({
            buildInputs = [
              rustNightly
              pkgs.cargo-fuzz
              pkgs.zlib
              pkgs.stdenv.cc.cc.lib
            ];
            shellHook = ''
              export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath [ pkgs.zlib pkgs.stdenv.cc.cc.lib ]}:$LD_LIBRARY_PATH"
              echo "===================================="
              echo " Welcome to the fuzzing shell!      "
              echo "===================================="
              cargo --version
              echo "The fuzz workspace lives in ./fuzz."
              echo "  cd fuzz && cargo fuzz run cluster_reference"
              echo "  cd fuzz && cargo fuzz run cluster_rotation"
              echo "Optionally: cargo fuzz run cluster_reference -- -max_total_time=240"
            '';
          });
          # nix run .#isolated [dir], see isolated.nix of nix-devtools
          apps.isolated = {
            type = "app";
            program = pkgs.lib.getExe isolated;
          };
        };
    };
}
