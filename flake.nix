{
  description = "Spawn debug shells in virtual machines";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    not-os.url = "github:cleverca22/not-os";
    not-os.flake = false;
    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, fenix, not-os }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        fenixPkgs = fenix.packages.${system};
        rustToolchain = with fenixPkgs; combine [
          latest.cargo
          latest.rustc
          latest.rust-src
          latest.rust-std
          latest.clippy-preview
          latest.rustfmt-preview
          targets.x86_64-unknown-linux-musl.latest.rust-std
          # fenix.packages.x86_64-linux.targets.aarch64-unknown-linux-gnu.latest.rust-std
        ];

        rustPlatform = (pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        });

        vmsh = pkgs.callPackage ./nix/vmsh.nix {
          pkgSrc = self;
          inherit rustPlatform;
        };

        kernel-fhs = pkgs.callPackage ./nix/kernel-fhs.nix {};


        ciDeps = [
          rustToolchain
          pkgs.qemu_kvm
          pkgs.tmux # needed for integration test
          (pkgs.python3.withPackages (ps: [
            ps.pytest
            ps.pytest-xdist
            ps.pyelftools
            ps.intervaltree

            # linting
            ps.black
            ps.flake8
            ps.isort
            ps.mypy
          ]))
        ] ++ vmsh.nativeBuildInputs;

        not-os-image = pkgs.callPackage ./nix/not-os-image.nix {
          inherit not-os;
        };
      in
      rec {
        # default target for `nix build`
        defaultPackage = vmsh;
        packages = {
          inherit vmsh;

          # used in .drone.yml
          ci-shell = pkgs.mkShell {
            inherit (vmsh) buildInputs KERNELDIR;
            nativeBuildInputs = ciDeps;
          };

          # see justfile/build-linux-shell
          kernel-fhs-shell = (kernel-fhs.override { runScript = "bash"; }).env;
          kernel-fhs = kernel-fhs;

          # see justfile/not-os
          inherit not-os-image;

          # see justfile/nixos-image
          nixos-image = pkgs.callPackage ./nix/nixos-image.nix {};
          busybox-image = pkgs.callPackage ./nix/busybox-image.nix {};
        };
        # used by `nix develop`
        devShell = pkgs.mkShell {
          inherit (vmsh) buildInputs;
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
          nativeBuildInputs = ciDeps ++ [
            pkgs.jq # needed for justfile
            pkgs.just
            pkgs.cargo-watch
            pkgs.cargo-deny
            pkgs.pre-commit
            pkgs.rls
            pkgs.rustracer
            pkgs.git # needed for pre-commit install
            fenixPkgs.rust-analyzer
            pkgs.gdb
          ];

          shellHook = ''
            pre-commit install
            export KERNELDIR=$(pwd)/../linux;
          '' + pkgs.lib.optionalString (false) ''
            # when debugging not-os kernel
            export KERNELDIR=${not-os-image.kerneldir};
          '';
          # interesting when supporting aarch64
          #CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER =
          #  "${pkgs.pkgsCross.aarch64-multiplatform.stdenv.cc}/bin/aarch64-unknown-linux-gnu-gcc";
        };
      });
}
