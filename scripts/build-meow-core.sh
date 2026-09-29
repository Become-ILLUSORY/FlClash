#!/usr/bin/env sh
# 构建 meow-rs 内核并为 Android 产出 libclash.so，替换 Go 内核产物。
#
# 用法:
#   ./scripts/build-meow-core.sh                   # 只构建当前主机 ABI 的不带 TUN 的最小集
#   ANDROID_NDK=<...> ./scripts/build-meow-core.sh --android
#
# 产物:
#   libclash/<target>/<abi>/libclash.so
#
# 前置:
#   - rustup 1.89+ / cargo（rust-toolchain.toml 会定版本）
#   - --android 需要 cargo-ndk（cargo install cargo-ndk）与 ANDROID_NDK
#   - meow-rs 子模块已初始化：git submodule update --init core/meow-rs

set -e
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [ ! -f core/meow-rs/Cargo.toml ]; then
  echo "error: core/meow-rs 子模块未初始化，请先：git submodule update --init --recursive" >&2
  exit 1
fi

FFI=core/meow-ffi
mkdir -p libclash/android/{arm64-v8a,armeabi-v7a,x86_64}

build_cargo() {
  local TARGET="$1" OUT="$2"
  [ -n "$TARGET" ] && CARGO_TARGET="--target $TARGET"
  (cd "$FFI" && cargo build --release $CARGO_TARGET --features full-proxy)
  cp "$FFI/target/$TARGET/release/libclash.so" "$OUT" 2>/dev/null \
    || cp "$FFI/target/release/libclash.so" "$OUT" 2>/dev/null
  echo ">>> $OUT"
}

if [ "$1" = "--android" ]; then
  : "${ANDROID_NDK:?need ANDROID_NDK set}"
  for abi in arm64-v8a armeabi-v7a x86_64; do
    case "$abi" in
      arm64-v8a) TARGET=aarch64-linux-android ;;
      armeabi-v7a) TARGET=armv7-linux-androideabi ;;
      x86_64) TARGET=x86_64-linux-android ;;
    esac
    (cd "$FFI" && cargo ndk --target "$TARGET" --platform 21 -- build --release --features full-proxy)
    cp "$FFI/target/$TARGET/release/libclash.so" "libclash/android/$abi/libclash.so"
  done
  # 头文件（供 core.cpp include）
  cp "$FFI/include/libclash.h" libclash/android/includes 2>/dev/null || true
  echo ">>> Android libclash.so 产物清单:"
  ls -l libclash/android/*/libclash.so
else
  build_cargo "" "libclash/linux/$(uname -m)/libclash.so"
fi