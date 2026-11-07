#!/usr/bin/env bash
# Build the minimal static OpenCV that the `opencv` crate links against.
#
# The same recipe the development machine uses (README "OpenCV 构建环境"): only core,
# imgproc, imgcodecs, features2d, calib3d and flann; static; no GUI, video or optional
# codecs. It runs on macOS and, from Git Bash with the MSVC environment loaded, on
# Windows. GitHub Actions caches the install prefix, so a cache hit only prints the
# environment.
#
#   scripts/ci/build-opencv.sh <install-prefix>
#
# After a build (or a cache hit) it prints the variables the `opencv` crate needs and,
# when $GITHUB_ENV is set, appends them there for the following workflow steps.
#
#   OPENCV_VERSION   default 4.12.0 (the version the golden fixtures were checked against)
#   OPENCV_WORK_DIR  where the source is downloaded and built (default $RUNNER_TEMP or /tmp)

set -euo pipefail

PREFIX="${1:?usage: build-opencv.sh <install-prefix>}"
VERSION="${OPENCV_VERSION:-4.12.0}"
WORK="${OPENCV_WORK_DIR:-${RUNNER_TEMP:-/tmp}/opencv-build}"
# SHA-256 of https://github.com/opencv/opencv/archive/refs/tags/4.12.0.tar.gz
SHA256_4_12_0="44c106d5bb47efec04e531fd93008b3fcd1d27138985c5baf4eafac0e1ec9e9d"

case "$VERSION" in
  4.12.0) EXPECTED_SHA="$SHA256_4_12_0" ;;
  *) echo "No pinned checksum for OpenCV $VERSION: add it to this script first." >&2; exit 1 ;;
esac

case "$(uname -s)" in
  Darwin) OS=macos ;;
  MINGW* | MSYS* | CYGWIN*) OS=windows ;;
  *) echo "Unsupported system: $(uname -s)" >&2; exit 1 ;;
esac

# Windows: the native tools (cmake, cl) want C:/..., Git Bash's own tools (tar, find) want
# /c/..., and a path like D:\a\_temp makes GNU tar look for a host called "D". WORK is kept
# in bash form and handed to cmake through native(); PREFIX is the C:/... form, which both accept.
native() {
  if [ "$OS" = windows ]; then cygpath -m "$1"; else printf '%s\n' "$1"; fi
}
if [ "$OS" = windows ]; then
  mkdir -p "$PREFIX"
  PREFIX="$(cygpath -m "$PREFIX")"
  mkdir -p "$WORK"
  WORK="$(cygpath -u "$WORK")"
fi

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

jobs() {
  case "$OS" in
    macos) sysctl -n hw.ncpu ;;
    windows) echo "${NUMBER_OF_PROCESSORS:-2}" ;;
  esac
}

# The build is described by this script and the version: change either and the stamp no
# longer matches, so a stale cache is rebuilt instead of trusted.
STAMP="$PREFIX/.liverguard-opencv-stamp"
STAMP_VALUE="$VERSION $(sha256 "$0")"

find_config_dir() {
  find "$PREFIX" -name OpenCVConfig.cmake -print -quit | xargs -I{} dirname {}
}

emit_env() {
  local dir
  dir="$(find_config_dir)"
  [ -n "$dir" ] || { echo "OpenCVConfig.cmake not found under $PREFIX" >&2; exit 1; }
  echo
  echo "OpenCV_DIR=$dir"
  echo "CMAKE_PREFIX_PATH=$PREFIX"
  echo "OPENCV_DISABLE_PROBES=pkg_config,vcpkg_cmake,vcpkg"
  if [ -n "${GITHUB_ENV:-}" ]; then
    {
      echo "OpenCV_DIR=$dir"
      echo "CMAKE_PREFIX_PATH=$PREFIX"
      echo "OPENCV_DISABLE_PROBES=pkg_config,vcpkg_cmake,vcpkg"
    } >>"$GITHUB_ENV"
  fi
}

if [ -f "$STAMP" ] && [ "$(cat "$STAMP")" = "$STAMP_VALUE" ]; then
  echo "OpenCV $VERSION already built in $PREFIX (cache hit)."
  emit_env
  exit 0
fi

rm -rf "$PREFIX" "$WORK/src" "$WORK/build"
mkdir -p "$PREFIX" "$WORK"
TARBALL="$WORK/opencv-$VERSION.tar.gz"

if [ ! -f "$TARBALL" ] || [ "$(sha256 "$TARBALL")" != "$EXPECTED_SHA" ]; then
  echo "Downloading OpenCV $VERSION..."
  curl -fL --retry 3 --retry-delay 5 -o "$TARBALL" "https://github.com/opencv/opencv/archive/refs/tags/$VERSION.tar.gz"
fi
ACTUAL_SHA="$(sha256 "$TARBALL")"
if [ "$ACTUAL_SHA" != "$EXPECTED_SHA" ]; then
  echo "Checksum mismatch for $TARBALL: expected $EXPECTED_SHA, got $ACTUAL_SHA" >&2
  exit 1
fi

mkdir -p "$WORK/src"
tar -xzf "$TARBALL" -C "$WORK/src" --strip-components=1

# Flags shared by every system: the module list is what keeps this small and fast.
FLAGS=(
  -DCMAKE_BUILD_TYPE=Release
  "-DCMAKE_INSTALL_PREFIX=$PREFIX"
  -DBUILD_LIST=core,imgproc,imgcodecs,features2d,calib3d,flann
  -DBUILD_SHARED_LIBS=OFF
  -DBUILD_TESTS=OFF -DBUILD_PERF_TESTS=OFF -DBUILD_EXAMPLES=OFF -DBUILD_DOCS=OFF
  -DBUILD_opencv_apps=OFF -DBUILD_JAVA=OFF -DBUILD_opencv_python3=OFF -DBUILD_opencv_python_bindings_generator=OFF
  -DWITH_FFMPEG=OFF -DWITH_OPENCL=OFF -DWITH_IPP=OFF -DWITH_ITT=OFF -DWITH_PROTOBUF=OFF -DWITH_OPENEXR=OFF
  -DWITH_WEBP=OFF -DWITH_TIFF=OFF -DWITH_JASPER=OFF -DWITH_OPENJPEG=OFF -DWITH_AVIF=OFF -DWITH_GDAL=OFF
  -DWITH_OBSENSOR=OFF -DWITH_EIGEN=OFF -DWITH_LAPACK=OFF -DWITH_QUIRC=OFF
  -DBUILD_PNG=ON -DBUILD_JPEG=ON -DBUILD_ZLIB=ON
  # Only the G-API module uses ADE, but 4.12 still downloads it and lists a `ade` library in
  # the exported targets without installing it, so every consumer's find_package fails.
  -DWITH_ADE=OFF
  -DCMAKE_POLICY_VERSION_MINIMUM=3.5
)

case "$OS" in
  macos)
    # ScreenCaptureKit needs macOS 14, and the app targets it (.cargo/config.toml).
    FLAGS+=(-DCMAKE_OSX_DEPLOYMENT_TARGET=14.0 -DWITH_AVFOUNDATION=OFF)
    ;;
  windows)
    # Rust's MSVC target links the dynamic CRT (/MD) unless told otherwise, and mixing
    # it with OpenCV's default static CRT (/MT) fails to link. Match Rust.
    FLAGS+=(-DBUILD_WITH_STATIC_CRT=OFF -DWITH_MSMF=OFF -DWITH_DSHOW=OFF -DBUILD_WITH_DEBUG_INFO=OFF)
    # Git Bash's /usr/bin/link (a coreutils tool) shadows MSVC's link.exe for CMake.
    rm -f /usr/bin/link.exe
    ;;
esac

cmake -G Ninja -S "$(native "$WORK/src")" -B "$(native "$WORK/build")" "${FLAGS[@]}"
cmake --build "$(native "$WORK/build")" --parallel "$(jobs)"
cmake --install "$(native "$WORK/build")"

# The exported targets must only name libraries that exist, or every consumer's
# find_package fails (this is how the ADE problem above showed up).
mkdir -p "$WORK/probe"
printf '%s\n' 'cmake_minimum_required(VERSION 3.5)' 'project(probe CXX)' \
  'find_package(OpenCV REQUIRED COMPONENTS core imgproc imgcodecs features2d calib3d flann)' \
  >"$WORK/probe/CMakeLists.txt"
if ! cmake -S "$(native "$WORK/probe")" -B "$(native "$WORK/probe/build")" \
    "-DOpenCV_DIR=$(find_config_dir)" >"$WORK/probe.log" 2>&1; then
  cat "$WORK/probe.log" >&2
  echo "The OpenCV installed in $PREFIX cannot be found with find_package." >&2
  exit 1
fi

echo "$STAMP_VALUE" >"$STAMP"
rm -rf "$WORK/src" "$WORK/build" "$WORK/probe" "$WORK/probe.log"
emit_env
