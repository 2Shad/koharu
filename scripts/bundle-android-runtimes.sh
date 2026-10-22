#!/usr/bin/env bash
# Copy the prebuilt native runtimes into the APK's jniLibs.
#
# `tauri android build` copies everything under jniLibs into the APK, and
# `Torch::AndroidCpu` / `Llama::AndroidCpu` dlopen these by bare soname through
# the app's native library directory. The activation loop loads every name in a
# package's `android` prop, so a missing file fails the whole package; the set
# below MUST stay identical to those props:
#   crates/koharu-runtime/src/runtime/packages/{torch,llama}.rs
#
# The runtimes are built outside this repository. Point the script at the two
# directories holding the built libraries, for example:
#   llama: libllama.so libmtmd.so
#   torch: libc10.so libtorch_global_deps.so libtorch_cpu.so libtorch.so
#          libkoharu-torch.so
#
# Usage: scripts/bundle-android-runtimes.sh <llama-dir> <torch-dir> [dest]
set -euo pipefail

llama_dir=${1:?usage: $0 <llama-dir> <torch-dir> [dest]}
torch_dir=${2:?usage: $0 <llama-dir> <torch-dir> [dest]}
root=$(cd "$(dirname "$0")/.." && pwd)
dest=${3:-"$root/crates/koharu/gen/android/app/src/main/jniLibs/arm64-v8a"}

sources=(
  "$llama_dir/libllama.so"
  "$llama_dir/libmtmd.so"
  "$torch_dir/libc10.so"
  "$torch_dir/libtorch_global_deps.so"
  "$torch_dir/libtorch_cpu.so"
  "$torch_dir/libtorch.so"
  "$torch_dir/libkoharu-torch.so"
)

missing=0
for source in "${sources[@]}"; do
  [ -f "$source" ] || { echo "missing: $source" >&2; missing=1; }
done
[ "$missing" -eq 0 ] || exit 1

mkdir -p "$dest"
for source in "${sources[@]}"; do
  cp -f "$source" "$dest/$(basename "$source")"
done

echo "bundled ${#sources[@]} libraries into $dest"
