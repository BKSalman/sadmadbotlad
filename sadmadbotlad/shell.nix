{ pkgs ? import <nixpkgs> { allowUnfree = true; }}: 
   pkgs.mkShell {
    buildInputs = with pkgs; [
      # unstable.mpv-unwrapped.dev
      # clang
      # llvmPackages.libclang
      # llvmPackages.libcxxClang
      # llvmPackages.bintools
      # yt-dlp
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

