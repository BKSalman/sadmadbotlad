{
  description = "sadmadbotlad flake";

  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, flake-utils, rust-overlay, crane, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; };

        libPath = with pkgs; lib.makeLibraryPath [
            openssl
            libiconv
            pkg-config
            rocksdb
            dbus
            mpv
        ];

        craneLib = (crane.mkLib nixpkgs.legacyPackages.${system});

        nativeBuildInputs = with pkgs; [
            dbus
            alsa-lib
            llvmPackages.libclang
            llvmPackages.libcxxClang
            makeWrapper
            playerctl
        ];

        buildInputs = with pkgs; [
            mold
            clang
            dbus
            mpv
        ];

        sadmadbotladArtifacts = craneLib.buildDepsOnly ({
          BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${pkgs.llvmPackages.libclang.lib}/lib/clang/${pkgs.lib.getVersion pkgs.clang}/include";
          ROCKSDB_LIB_DIR = "${pkgs.rocksdb}/lib/";
          ROCKSDB_STATIC = "true";
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          NIX_LDFLAGS="-l${pkgs.stdenv.cc.libcxx.cxxabi.libName}";

          pname = "sadmadbotlad";
          src = craneLib.cleanCargoSource (craneLib.path ./sadmadbotlad);
          inherit buildInputs nativeBuildInputs;
        });

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          targets = [ "wasm32-unknown-unknown" ];
        };

        frontendCraneLib = (craneLib.overrideToolchain rustToolchain).overrideScope' (final: prev: {
                  # inherit (import nixpkgs-for-wasm-bindgen { inherit system; }) wasm-bindgen-cli;
                });

        frontendArtifacts = frontendCraneLib.buildDepsOnly ({
          pname = "frontend";

          src = frontendCraneLib.cleanCargoSource (frontendCraneLib.path ./frontend);
          inherit buildInputs nativeBuildInputs;
          doCheck = false;
        });

        frontendPackage = with pkgs; frontendCraneLib.buildTrunkPackage {
              src = lib.cleanSourceWith {
                  src = ./frontend;
                  filter = path: type:
                    (lib.hasSuffix "\.html" path) ||
                    (lib.hasSuffix "\.css" path) ||
                    (lib.hasInfix "assets/" path) ||
                    # Default filter from crane (allow .rs files)
                    (frontendCraneLib.filterCargoSources path type)
                  ;
                };

              inherit buildInputs nativeBuildInputs;

              cargoArtifacts = frontendArtifacts;
            };

        serverArtifacts = craneLib.buildDepsOnly ({
          pname = "server";
          src = craneLib.cleanCargoSource (craneLib.path ./frontend/server);
          inherit buildInputs nativeBuildInputs;
        });
      in
        {
          packages = rec {
            sadmadbotlad = craneLib.buildPackage {
              BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${pkgs.llvmPackages.libclang.lib}/lib/clang/${pkgs.lib.getVersion pkgs.clang}/include";
              ROCKSDB_LIB_DIR = "${pkgs.rocksdb}/lib/";
              ROCKSDB_STATIC = "true";
              LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
              LD_LIBRARY_PATH = "${libPath}";

              src = craneLib.path ./sadmadbotlad;

              inherit buildInputs nativeBuildInputs;

              cargoArtifacts = sadmadbotladArtifacts;

              postInstall = ''
                patchelf --set-rpath ${libPath} $out/bin/sadmadbotlad

                wrapProgram $out/bin/sadmadbotlad \
                  --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.playerctl pkgs.yt-dlp ]}

                mkdir -p $out/share
                cp -r commands $out/share
              '';
            };

            frontend = frontendPackage;

            server = craneLib.buildPackage {
              src = craneLib.path ./frontend/server;

              inherit buildInputs nativeBuildInputs ;

              cargoArtifacts = serverArtifacts;
            };

            default = sadmadbotlad;
          };

          devShell = pkgs.mkShell.override { stdenv = pkgs.stdenvAdapters.useMoldLinker pkgs.stdenv; } {
            inherit buildInputs nativeBuildInputs;
            packages = with pkgs; [
              (rust-bin.stable.latest.default.override {
                extensions = [ "rust-src" "rust-analyzer" ];
                targets = [ "wasm32-unknown-unknown" ];
              })
            ];
            
            # NIX_LDFLAGS = "-l${pkgs.stdenv.cc.libcxx.cxxabi.libName}";
            BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${pkgs.llvmPackages.libclang.lib}/lib/clang/${builtins.elemAt (pkgs.lib.splitString "." (pkgs.lib.getVersion pkgs.clang)) 0}/include";
            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
            ROCKSDB_LIB_DIR = "${pkgs.rocksdb}/lib/";
            ROCKSDB_STATIC = "true";
            LD_LIBRARY_PATH = "${libPath}";
          };
      });
}

