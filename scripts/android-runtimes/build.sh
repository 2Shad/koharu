#!/usr/bin/env bash
# Build libllama.so + libmtmd.so for aarch64-linux-android from koharu-rs/llama's
# wrapper around upstream ggml-org/llama.cpp, pinned at tag b11642.
#
# Prereqs (already present on this host):
#   NDK r27c at $ANDROID_NDK_HOME, else $HOME/Android/Sdk/ndk/27.2.12479018
#   cmake 3.28.3, ninja 1.11.1
#
# Outputs are written under $ANDROID_RUNTIME_BUILD_ROOT, which defaults to
# $XDG_CACHE_HOME/koharu-android-runtimes (or ~/.cache/koharu-android-runtimes).
# Produces $ROOT/out/llama/{libllama.so,libmtmd.so}.
set -euo pipefail

ROOT="${ANDROID_RUNTIME_BUILD_ROOT:-${XDG_CACHE_HOME:-$HOME/.cache}/koharu-android-runtimes}"
WORK="$ROOT/work"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NDK=${ANDROID_NDK_HOME:-$HOME/Android/Sdk/ndk/27.2.12479018}
REV=b11642

# 1. upstream llama.cpp at the pinned tag
if [ ! -d "$WORK/llama.cpp" ]; then
  git clone --depth 1 --branch "$REV" https://github.com/ggml-org/llama.cpp.git "$WORK/llama.cpp"
fi

# 2. koharu-rs/llama cmake wrapper + the CPU-backend patch
mkdir -p "$WORK/pristine/cmake" "$WORK/wrapper/cmake"
for f in CMakeLists.txt ggml-static.cmake; do
  curl -sSL -o "$WORK/pristine/cmake/$f" "https://raw.githubusercontent.com/koharu-rs/llama/main/cmake/$f"
done
cp "$WORK/pristine/cmake/"* "$WORK/wrapper/cmake/"
(cd "$WORK" && patch -p1 -d wrapper -i "$HERE/llama-android-cpu-backend.patch")

# 3. configure + build (GGML_LLAMAFILE/GGML_OPENMP are forced OFF by the wrapper;
#    -Wl,-z,max-page-size=16384 is also added by the NDK toolchain itself)
cmake -S "$WORK/wrapper/cmake" -B "$WORK/build" -G Ninja \
  -DCMAKE_TOOLCHAIN_FILE="$NDK/build/cmake/android.toolchain.cmake" \
  -DANDROID_ABI=arm64-v8a \
  -DANDROID_PLATFORM=android-31 \
  -DCMAKE_BUILD_TYPE=Release \
  -DLLAMA_CPP_SOURCE_DIR="$WORK/llama.cpp" \
  -DLLAMA_BACKEND=CPU \
  -DGGML_CPU_KLEIDIAI=ON \
  -DCMAKE_SHARED_LINKER_FLAGS=-Wl,-z,max-page-size=16384

cmake --build "$WORK/build" --target llama mtmd -j 5

# 4. stage
mkdir -p "$ROOT/out/llama"
cp "$WORK/build/bin/libllama.so" "$WORK/build/bin/libmtmd.so" "$ROOT/out/llama/"
bash "$HERE/verify.sh" "$ROOT/out/llama"
