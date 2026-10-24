#!/usr/bin/env bash
# Verification for the aarch64-linux-android llama.cpp runtime artifacts.
set -uo pipefail
ROOT="${ANDROID_RUNTIME_BUILD_ROOT:-${XDG_CACHE_HOME:-$HOME/.cache}/koharu-android-runtimes}"
DIR="${1:-$ROOT/out/llama}"
NDK="${ANDROID_NDK_HOME:-$HOME/Android/Sdk/ndk/27.2.12479018}"
TOOL="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
READELF="$TOOL/llvm-readelf"
NM="$TOOL/llvm-nm"

for f in "$DIR/libllama.so" "$DIR/libmtmd.so"; do
  printf '\n===== %s =====\n' "$f"
  file "$f"
  ls -l "$f" | awk '{print "size:", $5, "bytes"}'
  printf -- '--- dynamic ---\n'
  "$READELF" -d "$f" | grep -E 'SONAME|NEEDED|RUNPATH|RPATH' || echo "(no SONAME/NEEDED entries)"
  printf -- '--- program headers (LOAD align) ---\n'
  "$READELF" -lW "$f" | grep -E 'LOAD|Align'
  printf -- '--- exported symbol counts ---\n'
  printf 'llama_*: %s\n' "$("$NM" -D --defined-only "$f" | awk '$3 ~ /^llama_/ {n++} END {print n+0}')"
  printf 'ggml_* : %s\n' "$("$NM" -D --defined-only "$f" | awk '$3 ~ /^ggml_/ {n++} END {print n+0}')"
  printf 'mtmd_* : %s\n' "$("$NM" -D --defined-only "$f" | awk '$3 ~ /^mtmd_/ {n++} END {print n+0}')"
done

printf '\n===== host path / bad dependency scan =====\n'
for f in "$DIR/libllama.so" "$DIR/libmtmd.so"; do
  echo "-- $f"
  "$READELF" -d "$f" | grep -E 'NEEDED' | grep -Ei 'ggml|gomp|omp|/home/|/usr/|/tmp/' && echo "!! BAD" || echo "ok: no libggml/libgomp/libomp/host paths"
done
