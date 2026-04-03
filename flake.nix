{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      treefmt-nix,
      ...
    }:
    let
      forEachSystem =
        f:
        nixpkgs.lib.genAttrs [
          "aarch64-darwin"
          "aarch64-linux"
          "x86_64-linux"
        ] (system: f system nixpkgs.legacyPackages.${system});

      mkToolchain =
        pkgs:
        (rust-overlay.lib.mkRustBin { } pkgs).fromRustupToolchainFile
          ./rust-toolchain.toml;

      mkTreefmt =
        pkgs:
        treefmt-nix.lib.evalModule pkgs {
          projectRootFile = "flake.nix";

          programs.nixfmt = {
            enable = true;
            width = 80;
          };

          programs.prettier.enable = true;

          programs.rustfmt = {
            enable = true;
            package = mkToolchain pkgs;
          };

          settings.formatter.prettier.options = [
            "--config"
            "${./prettier.config.json}"
          ];

          settings.formatter.rustfmt.options = [
            "--config-path"
            "${./rustfmt.toml}"
          ];
        };
    in
    {
      # Workaround for https://github.com/NixOS/nix/issues/8881 so that we
      # can run individual checks with `nix run .#check-<foo>`.
      apps = forEachSystem (
        system: pkgs:
        nixpkgs.lib.mapAttrs' (name: check: {
          name = "check-${name}";
          value = {
            type = "app";
            program =
              (pkgs.writeShellScript "check-${name}" ''
                # Force evaluation of ${check}.
                echo -e "\033[1;32m✓\033[0m Check '${name}' passed"
              '').outPath;
          };
        }) self.checks.${system}
      );

      devShells = forEachSystem (
        system: pkgs: {
          default = pkgs.mkShell {
            nativeBuildInputs = [
              ((mkToolchain pkgs).override {
                extensions = [
                  "clippy"
                  "rust-analyzer"
                  "rust-src"
                  "rustfmt"
                ];
              })
              (mkTreefmt pkgs).config.build.programs.prettier
              pkgs.nodejs
              pkgs.typescript-language-server
            ];
          };
        }
      );

      formatter = forEachSystem (
        _system: pkgs: (mkTreefmt pkgs).config.build.wrapper
      );

      checks = forEachSystem (
        _system: pkgs: {
          formatting = (mkTreefmt pkgs).config.build.check self;
        }
      );
    };
}
