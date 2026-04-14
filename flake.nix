{
  inputs = {
    crane.url = "github:ipetkov/crane";
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
      crane,
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
        ((rust-overlay.lib.mkRustBin { } pkgs.buildPackages).fromRustupToolchainFile ./rust-toolchain.toml)
        .override
          {
            targets = [ pkgs.stdenv.targetPlatform.rust.rustcTarget ];
          };

      mkCraneLib = pkgs: (crane.mkLib pkgs).overrideToolchain mkToolchain;

      mkNixbStoreBuildInputs =
        pkgs:
        let
          components =
            (pkgs.pkgsStatic.nixVersions.nixComponents_2_34.appendPatches [
              ./nix/patches/nix-store-optional-curl.patch
            ]).overrideScope
              (
                final: prev: {
                  nix-util = (prev.nix-util).overrideAttrs (old: {
                    propagatedBuildInputs = pkgs.lib.filter (
                      pkg: (pkg.pname or "") != "libarchive"
                    ) old.propagatedBuildInputs;
                    mesonFlags = old.mesonFlags ++ [
                      (pkgs.lib.mesonBool "archive-support" false)
                    ];
                  });
                  nix-util-c = prev.nix-util-c.override {
                    nix-util = final.nix-util;
                  };
                  nix-store =
                    (prev.nix-store.override {
                      nix-util = final.nix-util;
                      # nixpkgs currently enables the embedded sandbox shell for all
                      # static builds, but only wires the busybox sandbox shell path
                      # on Linux. Disable the embedded shell on non-Linux targets so
                      # the static package still builds.
                      embeddedSandboxShell = pkgs.stdenv.hostPlatform.isLinux && pkgs.stdenv.hostPlatform.isStatic;
                      # Local libstore use does not need authenticated S3 fetcher
                      # support, and disabling it keeps the static link closure
                      # smaller.
                      withAWS = false;
                    }).overrideAttrs
                      (old: {
                        buildInputs = pkgs.lib.remove prev.curl old.buildInputs;
                        mesonFlags = old.mesonFlags ++ [
                          (pkgs.lib.mesonBool "http-client" false)
                          (pkgs.lib.mesonBool "extra-store-implementations" false)
                        ];
                      });
                  nix-store-c = prev.nix-store-c.override {
                    nix-util-c = final.nix-util-c;
                    nix-store = final.nix-store;
                  };
                }
              );
        in
        [
          components.nix-util.dev
          components.nix-util-c.dev
          components.nix-store.dev
          components.nix-store-c.dev
        ];

      mkPackage =
        pkgs:
        let
          craneLib = mkCraneLib pkgs;
          nixbStoreBuildInputs = mkNixbStoreBuildInputs pkgs;
          src = craneLib.cleanCargoSource ./.;
          commonArgs = {
            inherit src;
            cargoVendorDir = craneLib.vendorCargoDeps { inherit src; };
            strictDeps = true;
            doCheck = false;
            CARGO_NET_GIT_FETCH_WITH_CLI = "true";
            PKG_CONFIG_ALL_STATIC = "1";
            nativeBuildInputs = [ pkgs.buildPackages.pkg-config ];
            buildInputs = nixbStoreBuildInputs;
          };
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        in
        craneLib.buildPackage (commonArgs // { inherit cargoArtifacts; });

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
      packages = forEachSystem (
        _system: pkgs:
        {
          default = mkPackage pkgs;
          aarch64-linux = mkPackage pkgs.pkgsCross.aarch64-multiplatform-musl;
          x86_64-linux = mkPackage pkgs.pkgsCross.musl64;
        }
        // nixpkgs.lib.optionalAttrs (pkgs.stdenv.buildPlatform.isDarwin) {
          aarch64-darwin = mkPackage pkgs.pkgsCross.aarch64-darwin;
          x86_64-darwin = mkPackage pkgs.pkgsCross.x86_64-darwin;
        }
      );

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
        _system: pkgs:
        let
          nixbStoreBuildInputs = mkNixbStoreBuildInputs pkgs;
        in
        {
          default = pkgs.mkShell {
            PKG_CONFIG_ALL_STATIC = "1";
            buildInputs = nixbStoreBuildInputs;
            nativeBuildInputs = [
              ((mkToolchain pkgs).override {
                extensions = [
                  "clippy"
                  "rust-analyzer"
                  "rust-src"
                  "rustfmt"
                ];
              })
              pkgs.buildPackages.pkg-config
              (mkTreefmt pkgs).config.build.programs.prettier
              pkgs.nodejs
              pkgs.typescript-language-server
            ];
          };
        }
      );

      formatter = forEachSystem (_system: pkgs: (mkTreefmt pkgs).config.build.wrapper);

      checks = forEachSystem (
        _system: pkgs: {
          formatting = (mkTreefmt pkgs).config.build.check self;
        }
      );
    };
}
