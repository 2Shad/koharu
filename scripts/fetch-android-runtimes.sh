#!/usr/bin/env bash
# Download and unpack the prebuilt native runtimes into the APK's jniLibs.
#
# libtorch and llama.cpp are ~320 MB of prebuilt libraries and are not in git, so
# without them the build publishes an app shell that stops at the detection load.
# A release tagged `android-runtimes` carries the set; see "Building the Android
# runtimes" in Android_build_plan.md.
#
# The asset is missing on ordinary branch and pull-request runs, where a compile
# check still has to pass, so a failed download is a notice unless `--require`
# asks for a hard failure. The runtimes below MUST stay identical to the
# `android` props in:
#   crates/koharu-runtime/src/runtime/packages/{torch,llama}.rs
#
# Usage: RUNTIMES_URL=<url> scripts/fetch-android-runtimes.sh <dest-dir> [--require]
set -euo pipefail

dest=${1:?usage: RUNTIMES_URL=<url> $0 <dest-dir> [--require]}
require=${2:-}
[ -z "$require" ] || [ "$require" = "--require" ] || { echo "unknown argument: $require" >&2; exit 2; }

url=${RUNTIMES_URL:?RUNTIMES_URL must name the runtime tarball}

libs=(
  libllama.so
  libmtmd.so
  libc10.so
  libtorch.so
  libtorch_cpu.so
  libtorch_global_deps.so
  libkoharu-torch.so
)

archive=${RUNNER_TEMP:-$(mktemp -d)}/runtimes.tar.gz
mkdir -p "$(dirname "$archive")"
if ! curl -fsSL "$url" -o "$archive"; then
  if [ "$require" = "--require" ]; then
    echo "no native runtime asset at $url" >&2
    exit 1
  fi
  echo "::notice::no native runtime asset at $url — publishing the app shell only"
  exit 0
fi

mkdir -p "$dest"
tar -xzf "$archive" -C "$dest"

missing=0
for lib in "${libs[@]}"; do
  [ -f "$dest/$lib" ] || { echo "missing runtime: $dest/$lib" >&2; missing=1; }
done
[ "$missing" -eq 0 ] || exit 1

ls -l "$dest"
echo "fetched ${#libs[@]} libraries into $dest"
