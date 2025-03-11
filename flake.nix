{
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
            snappy
            openssl
            libiconv
            pkg-config
            (rocksdb.override { enableLiburing = false; })
            dbus
            mpv
            llvmPackages.libclang
            llvmPackages.libcxxClang
        ];

        craneLib = crane.mkLib pkgs;

        nativeBuildInputs = with pkgs; [
            snappy
            dbus
            alsa-lib
            llvmPackages.libclang
            llvmPackages.libcxxClang
            clang
            makeWrapper
            playerctl
        ];

        buildInputs = with pkgs; [
            snappy
            dbus
            mpv
        ];

        sadmadbotladArtifacts = craneLib.buildDepsOnly ({
          pname = "sadmadbotlad";
          src = craneLib.cleanCargoSource ./sadmadbotlad;
          inherit buildInputs nativeBuildInputs;

          BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${pkgs.llvmPackages.libclang.lib}/lib/clang/${builtins.elemAt (pkgs.lib.splitString "." (pkgs.lib.getVersion pkgs.clang)) 0}/include";
          ROCKSDB_LIB_DIR = "${pkgs.rocksdb.override { enableLiburing = false; }}/lib/";
          SNAPPY_LIB_DIR = "${pkgs.snappy}/lib/";
          ROCKSDB_STATIC = "true";
          LIBCLANG_PATH = "${pkgs.clang.cc.lib}/lib";
          LD_LIBRARY_PATH = "${libPath}";
          # NIX_LDFLAGS = "-l${pkgs.llvmPackages.libcxx}";
        });

        frontendCraneLib = (crane.mkLib pkgs).overrideToolchain (p: p.rust-bin.stable.latest.default.override {
          targets = [ "wasm32-unknown-unknown" ];
        });

        frontendArtifacts = frontendCraneLib.buildDepsOnly ({
          pname = "frontend";

          src = frontendCraneLib.cleanCargoSource ./frontend;
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

          # The version of wasm-bindgen-cli here must match the one from Cargo.lock.
          # When updating to a new version replace the hash values with lib.fakeHash,
          # then try to do a build, which will fail but will print out the correct value
          # for `hash`. Replace the value and then repeat the process but this time the
          # printed value will be for the second `hash` below
          wasm-bindgen-cli = pkgs.buildWasmBindgenCli rec {
            src = pkgs.fetchCrate {
              pname = "wasm-bindgen-cli";
              version = "0.2.100";
              hash = "sha256-3RJzK7mkYFrs7C/WkhW9Rr4LdP5ofb2FdYGz1P7Uxog=";
              # hash = lib.fakeHash;
            };

            cargoDeps = pkgs.rustPlatform.fetchCargoVendor {
              inherit src;
              inherit (src) pname version;
              hash = "sha256-qsO12332HSjWCVKtf1cUePWWb9IdYUmT+8OPj/XP2WE=";
              # hash = lib.fakeHash;
            };
          };
        };

        serverArtifacts = craneLib.buildDepsOnly ({
          pname = "server";
          src = craneLib.cleanCargoSource ./frontend/server;
          inherit buildInputs nativeBuildInputs;
        });
      in
        {
          packages = rec {
            sadmadbotlad = craneLib.buildPackage {
              BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${pkgs.llvmPackages.libclang.lib}/lib/clang/${builtins.elemAt (pkgs.lib.splitString "." (pkgs.lib.getVersion pkgs.clang)) 0}/include";
              ROCKSDB_LIB_DIR = "${pkgs.rocksdb.override { enableLiburing = false; }}/lib/";
              SNAPPY_LIB_DIR = "${pkgs.snappy}/lib/";
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
            ROCKSDB_LIB_DIR = "${pkgs.rocksdb.override { enableLiburing = false; }}/lib/";
            SNAPPY_LIB_DIR = "${pkgs.snappy}/lib/";
            ROCKSDB_STATIC = "true";
            LIBCLANG_PATH = "${pkgs.clang.cc.lib}/lib";
            LD_LIBRARY_PATH = "${libPath}";
          };
      });
}

