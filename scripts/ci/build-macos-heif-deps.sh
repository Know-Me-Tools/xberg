#!/usr/bin/env bash
# Build the libheif decode stack (libde265, libaom, libheif) from source at
# MACOSX_DEPLOYMENT_TARGET, into a prefix that macOS artifacts bundle instead
# of Homebrew bottles, which target the runner's own macOS (#1243).
# Decode-only, mirroring the manylinux build-libheif step in xberg-io/actions. ~keep
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="${REPO_ROOT:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
source "$REPO_ROOT/scripts/lib/retry.sh"
source "$REPO_ROOT/scripts/lib/macho.sh"

LIBDE265_VERSION=1.1.3
LIBDE265_SHA256=554228bd17788c99a7e63b37ab5634722190e6e2bf60c1dcb01cef328e133905
LIBAOM_VERSION=3.15.2
LIBAOM_SHA256=67bb54b245f33ed98600e08269e6139986e48114e14042474ddd8885801dfddc
LIBHEIF_VERSION=1.23.7
LIBHEIF_SHA256=f7483e818503ea6224c8863e4a56ac02c7089021a98472c811c12aa19faf2475

PREFIX="${XBERG_HEIF_PREFIX:-/tmp/xberg-heif}"
TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"
WORK=""
JOBS="${CMAKE_BUILD_PARALLEL_LEVEL:-$(sysctl -n hw.ncpu)}"
if [[ ! "$JOBS" =~ ^[1-9][0-9]*$ ]]; then
  echo "::error::CMAKE_BUILD_PARALLEL_LEVEL must be a positive integer" >&2
  exit 1
fi

# Absolute install names so delocate can resolve every load command. ~keep
CMAKE_COMMON=(
  -DCMAKE_BUILD_TYPE=Release
  -DCMAKE_INSTALL_PREFIX="$PREFIX"
  -DCMAKE_INSTALL_LIBDIR=lib
  -DCMAKE_INSTALL_NAME_DIR="$PREFIX/lib"
  -DCMAKE_OSX_DEPLOYMENT_TARGET="$TARGET"
  -DBUILD_SHARED_LIBS=ON
)

ensure_nasm() {
  [ "$(uname -m)" = "x86_64" ] || return 0
  command -v nasm >/dev/null 2>&1 || retry_with_backoff brew install nasm
}

build_dep() {
  local name="$1" version="$2" url="$3" sha256="$4"
  shift 4
  echo "::group::$name $version"
  retry_with_backoff curl -fsSL --retry 3 -o "$WORK/$name.tar.gz" "$url"
  printf '%s  %s\n' "$sha256" "$WORK/$name.tar.gz" | shasum -a 256 -c -
  tar -xzf "$WORK/$name.tar.gz" -C "$WORK"
  cmake -S "$WORK/$name-$version" -B "$WORK/$name-build" "${CMAKE_COMMON[@]}" "$@"
  cmake --build "$WORK/$name-build" -j "$JOBS"
  cmake --install "$WORK/$name-build"
  echo "::endgroup::"
}

deps_of() {
  otool -L "$1" | tail -n +3 | sed 's/^[[:space:]]*//; s/ (compatibility.*//'
}

verify_dylib() {
  local lib="$1" minos dep
  minos="$(minos_of "$lib")"
  if [ "$minos" != "$TARGET" ]; then
    echo "::error::$(basename "$lib") targets macOS $minos, expected $TARGET"
    return 1
  fi
  while IFS= read -r dep; do
    case "$dep" in
    "$PREFIX"/lib/* | /usr/lib/* | /System/*) ;;
    *)
      echo "::error::$(basename "$lib") links outside the prefix: $dep"
      return 1
      ;;
    esac
  done < <(deps_of "$lib")
}

verify_prefix() {
  local lib fail=0 checked=0
  for lib in libheif libde265 libaom; do
    [ -f "$PREFIX/lib/$lib.dylib" ] || {
      echo "::error::missing $lib.dylib"
      return 1
    }
  done
  for lib in "$PREFIX"/lib/*.dylib; do
    [ -L "$lib" ] && continue
    checked=$((checked + 1))
    verify_dylib "$lib" || fail=1
  done
  [ "$checked" -ge 3 ] || {
    echo "::error::fewer than three real codec libraries were examined"
    return 1
  }
  return "$fail"
}

verify_versions() {
  local spec name expected actual
  for spec in "libheif:$LIBHEIF_VERSION" "libde265:$LIBDE265_VERSION" "aom:$LIBAOM_VERSION"; do
    name="${spec%%:*}"
    expected="${spec#*:}"
    actual="$(PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig" PKG_CONFIG_PATH="" pkg-config --modversion "$name")" || return 1
    if [ "$actual" != "$expected" ]; then
      echo "::error::$name version $actual does not match $expected" >&2
      return 1
    fi
  done
}

main() {
  local receipt
  receipt="libheif=$LIBHEIF_VERSION libde265=$LIBDE265_VERSION libaom=$LIBAOM_VERSION target=$TARGET arch=$(uname -m)"
  if [ -f "$PREFIX/lib/libheif.dylib" ] &&
    [ -f "$PREFIX/.xberg-heif-versions" ] &&
    [ "$(cat "$PREFIX/.xberg-heif-versions")" = "$receipt" ] && verify_versions; then
    verify_prefix
    echo "Verified libheif stack already present in $PREFIX, skipping build"
    return 0
  fi
  WORK="$(mktemp -d)"
  trap 'rm -rf "$WORK"' EXIT
  ensure_nasm

  build_dep libde265 "$LIBDE265_VERSION" \
    "https://github.com/strukturag/libde265/releases/download/v$LIBDE265_VERSION/libde265-$LIBDE265_VERSION.tar.gz" \
    "$LIBDE265_SHA256" -DENABLE_SDL=OFF

  # The macos-15-intel runner ships a nasm libaom's test_nasm rejects
  # ("multipass optimization not supported"), which aborts configure. This is a
  # decode-only build on an EOL arch, so drop to the generic (pure-C) CPU target
  # on x86_64 to bypass nasm entirely. arm64 keeps its NEON SIMD. ~keep
  aom_cpu_arg=""
  [ "$(uname -m)" = "x86_64" ] && aom_cpu_arg="-DAOM_TARGET_CPU=generic"
  build_dep libaom "$LIBAOM_VERSION" \
    "https://storage.googleapis.com/aom-releases/libaom-$LIBAOM_VERSION.tar.gz" \
    "$LIBAOM_SHA256" -DCONFIG_AV1_ENCODER=0 -DENABLE_APPS=0 -DENABLE_EXAMPLES=0 -DENABLE_TESTS=0 -DENABLE_DOCS=0 -DENABLE_TOOLS=0 \
    ${aom_cpu_arg}

  # Pin dependency discovery to this prefix so no Homebrew library leaks in. ~keep
  PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig" build_dep libheif "$LIBHEIF_VERSION" \
    "https://github.com/strukturag/libheif/releases/download/v$LIBHEIF_VERSION/libheif-$LIBHEIF_VERSION.tar.gz" \
    "$LIBHEIF_SHA256" -DCMAKE_PREFIX_PATH="$PREFIX" -DCMAKE_IGNORE_PREFIX_PATH="/opt/homebrew;/usr/local" \
    -DWITH_LIBDE265=ON -DWITH_AOM_DECODER=ON -DWITH_AOM_ENCODER=OFF -DWITH_X265=OFF \
    -DWITH_EXAMPLES=OFF -DWITH_GDK_PIXBUF=OFF -DBUILD_TESTING=OFF

  verify_versions
  verify_prefix
  printf '%s\n' "$receipt" >"$PREFIX/.xberg-heif-versions"
  echo "libheif stack built for macOS $TARGET in $PREFIX"
}

main "$@"
