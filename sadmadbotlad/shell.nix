let
  system = "x64_86-linux";
  unstable = import (fetchTarball https://github.com/nixos/nixpkgs/archive/nixpkgs-unstable.tar.gz) { };
  # masterpkgs = import builtins.fetchTarball https://github.com/nixos/nixpkgs/master.tar.gz {};
in
{ pkgs ? import <nixpkgs> { allowUnfree = true; }}: 
   pkgs.mkShell rec {
    buildInputs = with pkgs; [
      # unstable.mpv-unwrapped.dev
      # clang
      # llvmPackages.libclang
      # llvmPackages.libcxxClang
      # llvmPackages.bintools
      yt-dlp
      alsa-lib
      rocksdb
      mpv
      gcc
      openssl
      libiconv
      pkg-config
    ];
    LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
    BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${pkgs.llvmPackages.libclang.lib}/lib/clang/${pkgs.lib.getVersion pkgs.clang}/include";
    ROCKSDB_LIB_DIR = "${pkgs.rocksdb}/lib/";
    ROCKSDB_STATIC = "true";
  }

