{
  description = "sadmadbotlad flake";

  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem
      (system: let
        pkgs = import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; };
        libPath = pkgs.lib.makeLibraryPath [
            pkgs.openssl
            pkgs.libiconv
            pkgs.pkg-config
            pkgs.rocksdb
            pkgs.dbus
            pkgs.mpv
        ];
      in
        {
          devShell = pkgs.mkShell.override { stdenv = pkgs.stdenvAdapters.useMoldLinker pkgs.stdenv; } rec {
            NIX_CFLAGS_LINK = "-fuse-ld=mold";
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
                mold
                clang
                dbus
                mpv
                # llvmPackages.libclang
                # llvmPackages.libcxxClang
                # llvmPackages.bintools
            ];
              # LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
              BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${pkgs.llvmPackages.libclang.lib}/lib/clang/${pkgs.lib.getVersion pkgs.clang}/include";
              ROCKSDB_LIB_DIR = "${pkgs.rocksdb}/lib/";
              ROCKSDB_STATIC = "true";
              LD_LIBRARY_PATH = "${libPath}";
          };
      });
}

