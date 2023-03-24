{ pkgs ? import <nixpkgs> { }}: 
   pkgs.mkShell {
    nativeBuildInputs = with pkgs; [
      # mold
      clang
      llvmPackages.libclang
      llvmPackages.libcxxClang
      llvmPackages.bintools
      mpv
      # gcc
      openssl
      libiconv
      pkg-config
      rocksdb
      alsa-lib
    ];
    
    buildInputs = with pkgs; [
      yt-dlp
    ];
    # LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
    
    # BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${pkgs.llvmPackages.libclang.lib}/lib/clang/${pkgs.lib.getVersion pkgs.clang}/include";
    ROCKSDB_LIB_DIR = "${pkgs.rocksdb}/lib/";
    ROCKSDB_STATIC = "true";
  }
