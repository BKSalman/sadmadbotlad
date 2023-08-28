{
  description = "sadmadbotlad flake";

  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem
      (system: let
        rustOverlay = builtins.fetchTarball {
          url = "https://github.com/oxalica/rust-overlay/archive/master.tar.gz";
          sha256 = "04csw82q0y46y3bcpk645cfkid95q6ghnacw8b9x3lmwppwab686";
        };

        pkgs = import nixpkgs { inherit system; overlays = [ (import rustOverlay) ]; };
      in
        {
          devShell = pkgs.mkShell rec {
            packages = with pkgs; [
              (rust-bin.stable.latest.default.override {
                extensions = [ "rust-src" "rust-analyzer" ];
                targets = [ "wasm32-unknown-unknown" ];
              })
            ];
            
            nativebuildInputs = with pkgs; [
                dbus
                alsa-lib
            ];

            buildInputs = with pkgs; [
                dbus
                mpv
                clang
                llvmPackages.libclang
                llvmPackages.libcxxClang
                llvmPackages.bintools
                openssl
                libiconv
                pkg-config
                rocksdb
            ];
              LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
              BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${pkgs.llvmPackages.libclang.lib}/lib/clang/${pkgs.lib.getVersion pkgs.clang}/include";
              ROCKSDB_LIB_DIR = "${pkgs.rocksdb}/lib/";
              ROCKSDB_STATIC = "true";
              LD_LIBRARY_PATH = "${pkgs.lib.makeLibraryPath buildInputs}";
          };
      });
}

