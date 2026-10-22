#!/usr/bin/env bash
# Populate the Android APK's jniLibs with the prebuilt native runtimes.
#
# `tauri android build` copies everything under jniLibs into the APK, and
# `Torch::AndroidCpu` / `Llama::AndroidCpu` dlopen these by bare soname through
# the app's native library directory. The activation loop loads every name in a
# package's `android` prop, so a missing file fails the whole package; the set
# below MUST stay identical to those props:
#   crates/koharu-runtime/src/runtime/packages/{torch,llama}.rs
#
# The runtimes are built outside this repository (koharu-rs/torch and
# koharu-rs/llama). Point the script at a checkout with the two build trees:
#   <root>/torch/install/lib/          libc10.so libtorch_global_deps.so ...
#   <root>/torch/shim-install/         libkoharu-torch.so
#   <root>/llama/build/bin/            libllama.so libmtmd.so
#
# Usage: scripts/bundle-android-runtimes.sh <root> [dest]
set -euo pipefail

src=${1:?usage: $0 <koharu-rs-checkout> [dest]}
root=$(cd "$(dirname "$0")/.." && pwd)
dest=${2:-"$root/crates/koharu/gen/android/app/src/main/jniLibs/arm64-v8a"}

libraries=(
  "$src/torch/install/lib/libc10.so"
  "$src/torch/install/lib/libtorch_global_deps.so"
  "$src/torch/install/lib/libtorch_cpu.so"
  "$src/torch/install/lib/libtorch.so"
  "$src/torch/shim-install/libkoharu-torch.so"
  "$src/llama/build/bin/libllama.so"
  "$src/llama/build/bin/libmtmd.so"
)

mkdir -p "$dest"
for library in "${libraries[@]}"; do
  [ -f "$library" ] || { echo "missing: $library" >&2; exit 1; }
  ln -sf "$(readlink -f "$library")" "$dest/$(basename "$library")"
done

echo "bundled ${#libraries[@]} libraries into $dest"
