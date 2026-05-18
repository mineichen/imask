{
  description = "Deterministic Rust dev shell";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      perSystem = { system, pkgs, ... }:
        let
          rust = with inputs.fenix.packages.${system}; combine [
            stable.toolchain
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
          policy = pkgs.writeText "policy.json" ''{"default":[{"type":"insecureAcceptAnything"}]}'';
          containername = "imask-isolated-dev";
          podmanRun = "${pkgs.podman}/bin/podman run --rm -it "
            + "--network=slirp4netns "
            + "--tmpfs /tmp "
            + "-v ..:/workspace:z "
            + "-e HOME=/root "
            + "${containername}:latest /bin/entrypoint.sh";
        in
        {
          devShells.default = pkgs.mkShell({
            buildInputs = commonBuildInputs;
            shellHook = greet;
          });
          packages.isolated-build = pkgs.dockerTools.buildImage {
            name = containername;
            tag = "latest";
            copyToRoot = pkgs.buildEnv {
              name = containername;
              paths = commonBuildInputs ++ [
                pkgs.bashInteractive
                pkgs.ripgrep
                pkgs.git
                pkgs.opencode
                pkgs.busybox
                (pkgs.writeScriptBin "entrypoint.sh" ''
                  #!${pkgs.bashInteractive}/bin/bash
                  ${greet}
                  exec ${pkgs.bashInteractive}/bin/bash
                '')
              ];
              pathsToLink = [ "/bin" "/lib" "/include" "/share" ];
            };
            config = {
              Env = [ "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" "HOME=/root" ];
              Cmd = [ "/bin/entrypoint.sh" ];
              WorkingDir = "/workspace/imask";
            };
          };
          apps.isolated-build = {
            type = "app";
            program = toString (pkgs.writeShellScript containername ''
              ${pkgs.podman}/bin/podman rmi ${containername} || true
              ${pkgs.podman}/bin/podman load \
                --signature-policy ${policy} \
                --input ${inputs.self.packages.${system}.isolated-build}
              ${podmanRun}
            '');
          };
          apps.isolated-nobuild = {
            type = "app";
            program = toString (pkgs.writeShellScript "run-isolated" ''
              set -euo pipefail
              #if ! ${pkgs.podman}/bin/podman image exists ${containername}:latest 2>/dev/null; then
              #  echo "Image ${containername}:latest not found."
              #  echo "Please build and load it first with:"
              #  echo "  nix run .#isolated-build"
              #  exit 1
              #fi
              ${podmanRun}
            '');
          };
        };
    };
}
