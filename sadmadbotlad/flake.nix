{
  description = "sadmadbotlad flake";

  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem
      (system: let
        pkgs = import nixpkgs { inherit system; };
      in
        {
          devShell = pkgs.mkShell rec {
            packages = with pkgs; [
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

