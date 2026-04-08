{
  inputs = {
    crate2nix = {
      url = "github:nix-community/crate2nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
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
      crate2nix,
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
        ((rust-overlay.lib.mkRustBin { } pkgs.buildPackages).fromRustupToolchainFile
          ./rust-toolchain.toml
        ).override
          {
            targets = [ pkgs.stdenv.hostPlatform.rust.rustcTarget ];
          };

      mkCargoNix =
        pkgs:
        let
          crate2nixToolsBase = pkgs.callPackage "${crate2nix}/tools.nix" { };
          parseGitSource = crate2nixToolsBase.internal.parseGitSource;

          fetchGitRepo =
            source:
            let
              parsed = parseGitSource source;
            in
            builtins.fetchGit (
              {
                submodules = true;
                inherit (parsed) url;
                rev = if parsed.urlFragment == null then parsed.rev else parsed.urlFragment;
              }
              // (
                if (parsed ? branch || parsed ? tag) then
                  { ref = parsed.branch or "refs/tags/${parsed.tag}"; }
                else
                  { allRefs = true; }
              )
            );

          fetchGitWithBuiltins =
            args:
            let
              src = fetchGitRepo "git+${args.url}#${args.rev}";
            in
            src // { name = args.name or (builtins.baseNameOf args.url); };

          mkPkgsWithSshGit =
            cargoPkgs:
            cargoPkgs
            // {
              fetchgit = fetchGitWithBuiltins;
            }
            // nixpkgs.lib.optionalAttrs (cargoPkgs ? buildPackages) {
              buildPackages = cargoPkgs.buildPackages // {
                fetchgit = fetchGitWithBuiltins;
              };
            };

          cargoPkgs = mkPkgsWithSshGit pkgs;
          crate2nixTools = import "${crate2nix}/tools.nix" {
            pkgs = cargoPkgs;
          };
          generatedCargoNix = crate2nixTools.generatedCargoNix {
            name = "cache-action";
            src = ./.;
          };
        in
        import generatedCargoNix {
          pkgs = cargoPkgs;
          buildRustCrateForPkgs =
            cratePkgs:
            cratePkgs.buildRustCrate.override {
              cargo = mkToolchain cratePkgs;
              rustc = mkToolchain cratePkgs;
            };
        };

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
        let
          mkPackage = pkgs: (mkCargoNix pkgs).rootCrate.build;
        in
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
        _system: pkgs: {
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
