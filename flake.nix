{
  description = "sensors.rs - a fast, dependency-free lm-sensors' sensors(1) for macOS and Linux";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        sensors = pkgs.rustPlatform.buildRustPackage {
          pname = "sensors-rs";
          version = cargoToml.package.version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          buildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.darwin.apple_sdk.frameworks.IOKit
            pkgs.darwin.apple_sdk.frameworks.CoreFoundation
          ];

          meta = with pkgs.lib; {
            description = cargoToml.package.description;
            homepage = cargoToml.package.repository;
            license = licenses.mit;
            mainProgram = "sensors";
            platforms = platforms.darwin ++ platforms.linux;
          };
        };
      in
      {
        packages.default = sensors;
        packages.sensors-rs = sensors;

        apps.default = flake-utils.lib.mkApp {
          drv = sensors;
          name = "sensors";
        };

        devShells.default = pkgs.mkShell {
          packages = [ pkgs.cargo pkgs.rustc pkgs.rustfmt pkgs.clippy ];
        };
      });
}
